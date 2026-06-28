// Transplanted from hugit/crates/hugit-runner @ ead800d83d19bfd7f90bf4241ee27b18b09007f1 (runner-transfer campaign R2, 2026-06-10) — wire-contract seam, no git dep.
//! Cache-warm boot (WP-C3).
//!
//! Hydrates a runner job's toolchain layers from the CAS/AC on lease, so that a
//! warm boot (all layers already cached) completes in ≤10s while a cold boot
//! (layers not cached) takes ≥60s because it must materialize from the CAS.
//! Toolchain layers are **content-addressed** CAS objects shared across jobs:
//! one physical copy per content hash, never per-job duplication.
//!
//! # Owned items (WP-C3)
//! ① Warm ≤10s vs cold ≥60s — the warm path skips all CAS fetches; the cold
//!    path materializes every layer from the CAS/registry.
//! ② Toolchain layers shared — the same content key is served from cache across
//!    concurrent jobs (content-addressed, one physical copy per digest).
//! ③ CAS/AC down mid-job → fail CLOSED — [`BootError::SubstrateDown`] is
//!    returned, zero bytes are written to the CAS/AC, there is no hang (the CAS
//!    seam returns immediately), and no false green is emitted (§9 lock 5).
//!
//! # Architecture
//! - [`BootCas`] — the seam over the CAS/AC. The acceptance suite uses
//!   `FakeCas`; the live path uses `BoxHydrate`.
//! - [`ToolchainLayer`] — one toolchain content chunk, keyed by content hash.
//! - [`HydrationPlan`] — the set of layers a lease needs, plus its fence.
//! - [`hydrate`] — warm path: skips layers already in cache (zero fetches when warm).
//! - [`cold_hydrate`] — cold path: forces a fresh fetch of every layer.
//! - [`BootOutcome`] — success result: `Hydrated { layers_fetched }`.
//! - [`BootError`] — failure result, including `SubstrateDown` for §9 lock 5.
//!
//! # Container naming
//! All C3 hydration is scoped to the lease's `lease_id`. It does not create its
//! own containers — it operates on layers that are placed into the CAS/AC before
//! a container is started by the C2a runtime.
//!
//! # clw hydrate
//! In production, `clw hydrate` is the snapshot/hydrate/run reference client.
//! The [`BoxHydrate`] surface drives `clw hydrate` on the box for the live path
//! (box-lane only). Do NOT re-implement hydration; instead this module calls
//! `clw hydrate` via the box transport.
//!
//! # Fail-closed on substrate loss (§9 lock 5)
//! If CAS or AC is unreachable mid-job, the job fails CLOSED:
//! - [`BootError::SubstrateDown`] is returned with the substrate name and reason.
//! - Zero bytes are written to the CAS/AC (the write path is never entered on
//!   fetch failure; write failure surfaces immediately without retry).
//! - No indefinite hang (the seam returns an error immediately; no blocking retry).
//! - No false green (callers receive `Err`, never `Ok(BootOutcome::Hydrated)`).

use corelink_runners_contracts::FenceManifest;

use crate::lease::BoxExec;

// ── BootError ────────────────────────────────────────────────────────────────

/// Error type for the cache-warm boot path.
#[derive(Debug)]
pub enum BootError {
    /// CAS or AC substrate is unreachable mid-job (§9 lock 5 degradation).
    ///
    /// **Fail-closed invariants:**
    /// - Zero bytes written to the substrate before this error is returned.
    /// - No indefinite hang: the seam returns immediately.
    /// - No false green: the caller receives `Err`, never `Ok`.
    SubstrateDown {
        /// Which substrate is down (`"CAS"` or `"AC"`).
        substrate: String,
        /// Human-readable reason for the failure.
        reason: String,
    },
    /// A required toolchain layer is not available and cannot be fetched.
    LayerUnavailable {
        /// The content key of the layer that could not be fetched.
        content_key: String,
        /// Human-readable reason.
        reason: String,
    },
    /// The hydration plan is invalid (e.g. empty lease id, malformed keys).
    InvalidPlan {
        /// Human-readable reason.
        reason: String,
    },
}

impl std::fmt::Display for BootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BootError::SubstrateDown { substrate, reason } => {
                write!(
                    f,
                    "substrate {substrate:?} is down mid-job (§9 lock 5 — fail CLOSED): {reason}"
                )
            }
            BootError::LayerUnavailable {
                content_key,
                reason,
            } => {
                write!(
                    f,
                    "toolchain layer {content_key:?} is unavailable (fail CLOSED): {reason}"
                )
            }
            BootError::InvalidPlan { reason } => {
                write!(f, "invalid hydration plan (fail CLOSED): {reason}")
            }
        }
    }
}

impl std::error::Error for BootError {}

// ── ToolchainLayer ────────────────────────────────────────────────────────────

/// One toolchain content chunk in the CAS.
///
/// Layers are content-addressed: the `content_key` is a `sha256:<hex>` digest
/// that uniquely identifies the byte content. Sharing is by identity: two jobs
/// that need the same toolchain layer reference the same `content_key` and share
/// the single physical CAS object — no per-job duplication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolchainLayer {
    /// Content-addressed key (`sha256:<64-hex>`).
    pub content_key: String,
    /// Size of the layer in bytes (informational; not used for integrity checks
    /// in this module — the CAS is the source of truth).
    pub size_bytes: u64,
}

// ── HydrationPlan ─────────────────────────────────────────────────────────────

/// What layers a lease needs for its toolchain.
///
/// Derived from the frozen [`RunnerLease`](corelink_runners_contracts::RunnerLease) and
/// the [`FenceManifest`] (which scopes the materialized view). Fence
/// enforcement (ENOENT for paths outside `path_set`) is C5a's concern — C3
/// uses the fence only to scope the set of layers it hydrates.
#[derive(Debug, Clone)]
pub struct HydrationPlan {
    /// Unique lease identifier (must be non-empty).
    pub lease_id: String,
    /// Ordered list of toolchain layers to hydrate.
    pub toolchain_layers: Vec<ToolchainLayer>,
    /// Fence manifest scoping this hydration.
    pub fence: FenceManifest,
}

// ── BootCas ───────────────────────────────────────────────────────────────────

/// Seam over the CAS/AC consumed by the hydrate path.
///
/// The real implementation drives the CoreLink CAS via network calls; the test
/// oracle uses `FakeCas`. The seam must be:
/// - **Fail-closed on substrate loss**: return `Err(BootError::SubstrateDown)`
///   immediately (no blocking retry loop), with zero prior writes.
/// - **Content-addressed**: `fetch_layer` must serve the same bytes for the
///   same `content_key`, regardless of which job requests it.
pub trait BootCas {
    /// `true` iff the layer identified by `content_key` is already in the
    /// local/box cache (no fetch needed on the warm path).
    fn is_cached(&self, layer_key: &str) -> bool;

    /// Fetch the layer bytes for `layer_key` from the CAS origin.
    ///
    /// # Return convention
    /// - **CAS hit (200/2xx):** returns `Ok(Some(bytes))` — layer is present, warm path.
    /// - **CAS miss (404):** returns `Ok(None)` — layer is absent; the cold path
    ///   proceeds. A miss is NOT an error ("cache absent ⇒ slow, never broken",
    ///   A5/A1) — but it is DISTINCT from a present-but-empty layer, so the caller
    ///   never writes a miss-sentinel back into the CAS (the A1 poison bug). On a
    ///   `None` the caller skips the write-back and does NOT mark the key cached.
    /// - **CAS unreachable / auth failure (401/403/5xx/transport err):**
    ///   returns `Err(BootError::SubstrateDown)` — hard fail-closed (A5/A5b/A12).
    ///   An auth outage or server error is NEVER silently treated as a cold miss.
    fn fetch_layer(&self, layer_key: &str) -> Result<Option<Vec<u8>>, BootError>;

    /// Write (cache) a fetched layer so future jobs can reuse it without
    /// re-fetching from the origin.
    ///
    /// This is the **only** write path in the hydrate flow. It must:
    /// - Return `Err(BootError::SubstrateDown)` immediately if AC is down (no
    ///   partial write, no blocking retry).
    /// - Never be called if `fetch_layer` failed (the fetch failure aborts the
    ///   hydrate before this is reached).
    ///
    /// # Errors
    /// Returns `BootError::SubstrateDown` if the AC is unreachable.
    fn write_layer(&self, layer_key: &str, data: &[u8]) -> Result<(), BootError>;
}

// ── BootOutcome ───────────────────────────────────────────────────────────────

/// Successful result of a hydration run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootOutcome {
    /// All required layers are materialized and ready.
    Hydrated {
        /// Number of layers fetched from the CAS origin. `0` on the warm path
        /// (all layers were already cached). Positive on the cold path.
        layers_fetched: usize,
    },
    /// Layers were fetched but write-back to the AC/CAS failed (e.g. AC
    /// temporarily unreachable). The run proceeds but is RECORDED as a
    /// forced-cold: it is NOT a cache hit, and the result is NOT stored for
    /// future warm hits. Honest accounting — a poisoned store is never
    /// written (A5b asymmetry: AC-unreachable ⇒ forced-cold, not a hit;
    /// CAS-unreachable mid-fetch ⇒ `Err(SubstrateDown)` hard fail-closed).
    ForcedCold {
        /// Number of layers fetched (before the write-back failure).
        layers_fetched: usize,
        /// Human-readable reason the write-back was skipped.
        reason: String,
    },
    /// Hydration failed with a defined error status. This variant is for
    /// internal use; callers receive `Err(BootError)` — `Failed` is never
    /// returned as `Ok`. It exists so the seam can represent pre-return state
    /// during development without a silent panic.
    Failed {
        /// Human-readable failure reason.
        reason: String,
    },
}

// ── hydrate (warm path) ───────────────────────────────────────────────────────

/// Warm-path hydrate: materialize only the layers **not** already in cache.
///
/// On a warm box (all toolchain layers cached from a prior job), this function
/// performs zero CAS fetches — the structural cause of the ≤10s warm boot. On a
/// cold box, it fetches only the missing layers.
///
/// # Fail-closed on substrate loss (§9 lock 5)
/// If `fetch_layer` or `write_layer` returns [`BootError::SubstrateDown`], this
/// function propagates that error immediately. No further fetches or writes are
/// attempted. Zero bytes are written to the CAS/AC after a fetch failure.
///
/// # Errors
/// Returns `BootError::SubstrateDown` if the CAS or AC is unreachable mid-job.
/// Returns `BootError::InvalidPlan` if the plan is invalid.
pub fn hydrate<C: BootCas>(cas: &C, plan: &HydrationPlan) -> Result<BootOutcome, BootError> {
    validate_plan(plan)?;

    let mut layers_fetched: usize = 0;

    for layer in &plan.toolchain_layers {
        if cas.is_cached(&layer.content_key) {
            // Warm hit: the layer is already in the box/local cache.
            // No fetch, no write — this is the structural cause of the warm speedup.
            continue;
        }

        // Cold miss: fetch from the CAS origin.
        // ORDERING: fetch first (no write on fetch failure — zero poisoned writes).
        // CAS-unreachable → hard fail-closed (A5b: SubstrateDown propagated).
        // A 404 MISS → `None`: the layer is absent; proceed cold for it WITHOUT
        // writing a miss-sentinel back (the A1 poison bug) and WITHOUT marking it
        // cached. "miss ≠ broken" — the run continues, this layer just stays cold.
        let Some(data) = cas.fetch_layer(&layer.content_key)? else {
            continue;
        };

        // Write (cache) the fetched layer for future jobs.
        // AC-unreachable → ForcedCold (A5b asymmetry: see cold_hydrate docs).
        if let Err(e) = cas.write_layer(&layer.content_key, &data) {
            return Ok(BootOutcome::ForcedCold {
                layers_fetched,
                reason: format!(
                    "write-back failed after {layers_fetched} layer(s) (AC unreachable — \
                     forced-cold, not a hit; A5b): {e}"
                ),
            });
        }

        layers_fetched += 1;
    }

    Ok(BootOutcome::Hydrated { layers_fetched })
}

// ── cold_hydrate (cold path) ──────────────────────────────────────────────────

/// Cold-path hydrate: force-fetch ALL layers from the CAS, regardless of cache.
///
/// Used when the box is cold (no cached layers) or when a fresh materialization
/// is required (e.g. after layer eviction). This path always performs
/// `plan.toolchain_layers.len()` CAS fetches — the structural cause of the
/// ≥60s cold boot.
///
/// # Fail-closed on fetch failure (§9 lock 5 — CAS-unreachable)
/// If `fetch_layer` returns [`BootError::SubstrateDown`] (CAS unreachable, A5b),
/// the error is propagated immediately. No further fetches or writes are
/// attempted. This is the hard fail-closed path — no half-hydrated box proceeds.
///
/// # Write-back failure → ForcedCold (AC-unreachable asymmetry, A5b)
/// If `write_layer` returns [`BootError::SubstrateDown`] (AC unreachable), the
/// run is NOT aborted — the fetch already succeeded and the layer is available
/// for this job. Instead, the outcome is recorded as
/// [`BootOutcome::ForcedCold`]: the run proceeds but is NOT a cache hit (the
/// result is not stored for future warm hits). This is honest accounting: the
/// runner never silently stores a partial or poisoned write.
///
/// # Errors
/// Returns `BootError::SubstrateDown` if the CAS is unreachable mid-fetch.
/// Returns `BootError::InvalidPlan` if the plan is invalid.
pub fn cold_hydrate<C: BootCas>(cas: &C, plan: &HydrationPlan) -> Result<BootOutcome, BootError> {
    validate_plan(plan)?;

    let mut layers_fetched: usize = 0;

    for layer in &plan.toolchain_layers {
        // ORDERING: fetch first (no write on fetch failure — zero poisoned writes).
        // CAS-unreachable here → hard fail-closed (A5b: SubstrateDown propagated).
        // A 404 MISS → `None`: proceed cold for this layer WITHOUT writing a
        // miss-sentinel back (A1 poison) and WITHOUT marking it cached.
        let Some(data) = cas.fetch_layer(&layer.content_key)? else {
            continue;
        };

        // Write (cache) the fetched layer.
        // AC-unreachable here → ForcedCold (A5b asymmetry: fetch succeeded, write
        // failed; run proceeds but is recorded as forced-cold, NOT a hit).
        if let Err(e) = cas.write_layer(&layer.content_key, &data) {
            return Ok(BootOutcome::ForcedCold {
                layers_fetched,
                reason: format!(
                    "write-back failed after {layers_fetched} layer(s) (AC unreachable — \
                     forced-cold, not a hit; A5b): {e}"
                ),
            });
        }

        layers_fetched += 1;
    }

    Ok(BootOutcome::Hydrated { layers_fetched })
}

// ── BoxHydrate (live box path) ────────────────────────────────────────────────

/// Live-box hydrate driver: calls `clw hydrate` on the runner box via the
/// [`BoxExec`] transport.
///
/// Used by the box-dependent lane of item ① (timing assertion). The hermetic
/// items (②, ③, and the structural part of ①) use the in-process fake CAS
/// directly and do not go through this driver.
///
/// `clw` is THE snapshot/hydrate/run reference client (WP-C3 implementation
/// note). This driver does NOT re-implement hydration — it calls `clw hydrate`
/// with the plan's layer keys as arguments and interprets the exit code.
pub struct BoxHydrate;

impl BoxHydrate {
    /// Warm-path hydrate on the live box.
    ///
    /// Assumes the box already has the layers cached (pre-pulled by suite
    /// setup). Calls `clw hydrate --warm <layer-keys...>` and returns an error
    /// if the exit code is non-zero.
    ///
    /// # Errors
    /// Returns an error string if the box command fails.
    pub fn hydrate_warm<B: BoxExec>(boxx: &B, plan: &HydrationPlan) -> Result<BootOutcome, String> {
        Self::run_clw_hydrate(boxx, plan, false)
    }

    /// Cold-path hydrate on the live box.
    ///
    /// Forces a fresh materialization from the CAS/registry. Calls
    /// `clw hydrate --cold <layer-keys...>` and returns an error if the exit
    /// code is non-zero.
    ///
    /// # Errors
    /// Returns an error string if the box command fails.
    pub fn hydrate_cold<B: BoxExec>(boxx: &B, plan: &HydrationPlan) -> Result<BootOutcome, String> {
        Self::run_clw_hydrate(boxx, plan, true)
    }

    /// Drive `clw hydrate [--cold] <keys...>` on the box.
    fn run_clw_hydrate<B: BoxExec>(
        boxx: &B,
        plan: &HydrationPlan,
        cold: bool,
    ) -> Result<BootOutcome, String> {
        let mut argv = vec!["clw", "hydrate"];
        if cold {
            argv.push("--cold");
        }
        // Pass the layer keys as positional arguments.
        let keys: Vec<&str> = plan
            .toolchain_layers
            .iter()
            .map(|l| l.content_key.as_str())
            .collect();
        argv.extend_from_slice(&keys);

        let out = boxx
            .run(&argv)
            .map_err(|e| format!("clw hydrate failed to spawn on box: {e}"))?;

        if out.ok() {
            let layers_fetched = if cold { plan.toolchain_layers.len() } else { 0 };
            Ok(BootOutcome::Hydrated { layers_fetched })
        } else {
            Err(format!(
                "clw hydrate exited {:?} (stderr: {})",
                out.code,
                out.stderr.trim()
            ))
        }
    }
}

// ── validate_plan ─────────────────────────────────────────────────────────────

/// Validate a [`HydrationPlan`] before any CAS interaction.
///
/// # Errors
/// Returns [`BootError::InvalidPlan`] if:
/// - `lease_id` is empty.
/// - Any layer `content_key` is empty.
fn validate_plan(plan: &HydrationPlan) -> Result<(), BootError> {
    if plan.lease_id.trim().is_empty() {
        return Err(BootError::InvalidPlan {
            reason: "lease_id is empty".to_string(),
        });
    }
    for layer in &plan.toolchain_layers {
        if layer.content_key.trim().is_empty() {
            return Err(BootError::InvalidPlan {
                reason: format!(
                    "toolchain layer has an empty content_key (layer index {})",
                    plan.toolchain_layers
                        .iter()
                        .position(|l| l.content_key.trim().is_empty())
                        .unwrap_or(0)
                ),
            });
        }
    }
    Ok(())
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    struct AllCachedFake;
    impl BootCas for AllCachedFake {
        fn is_cached(&self, _key: &str) -> bool {
            true
        }
        fn fetch_layer(&self, key: &str) -> Result<Option<Vec<u8>>, BootError> {
            Err(BootError::SubstrateDown {
                substrate: "CAS".to_string(),
                reason: format!("AllCachedFake: fetch should never be called for {key:?}"),
            })
        }
        fn write_layer(&self, key: &str, _data: &[u8]) -> Result<(), BootError> {
            Err(BootError::SubstrateDown {
                substrate: "AC".to_string(),
                reason: format!("AllCachedFake: write should never be called for {key:?}"),
            })
        }
    }

    struct NoCacheFake {
        write_count: std::cell::Cell<usize>,
    }
    impl NoCacheFake {
        fn new() -> Self {
            Self {
                write_count: std::cell::Cell::new(0),
            }
        }
    }
    impl BootCas for NoCacheFake {
        fn is_cached(&self, _key: &str) -> bool {
            false
        }
        fn fetch_layer(&self, key: &str) -> Result<Option<Vec<u8>>, BootError> {
            Ok(Some(key.as_bytes().to_vec()))
        }
        fn write_layer(&self, _key: &str, _data: &[u8]) -> Result<(), BootError> {
            self.write_count.set(self.write_count.get() + 1);
            Ok(())
        }
    }

    fn sample_plan_unit() -> HydrationPlan {
        HydrationPlan {
            lease_id: "test-lease-unit".to_string(),
            toolchain_layers: vec![
                ToolchainLayer {
                    content_key: "sha256:aa".repeat(32),
                    size_bytes: 100,
                },
                ToolchainLayer {
                    content_key: "sha256:bb".repeat(32),
                    size_bytes: 200,
                },
            ],
            fence: FenceManifest {
                path_set: vec!["src/".to_string()],
                deny_default: true,
                materialized: vec![],
            },
        }
    }

    #[test]
    fn warm_path_zero_fetches_zero_writes() {
        let plan = sample_plan_unit();
        let cas = AllCachedFake;
        let out = hydrate(&cas, &plan).expect("warm hydrate must succeed");
        assert_eq!(
            out,
            BootOutcome::Hydrated { layers_fetched: 0 },
            "warm path: all cached → zero fetches"
        );
    }

    #[test]
    fn cold_path_fetches_all_layers() {
        let plan = sample_plan_unit();
        let cas = NoCacheFake::new();
        let out = cold_hydrate(&cas, &plan).expect("cold hydrate must succeed");
        assert_eq!(
            out,
            BootOutcome::Hydrated { layers_fetched: 2 },
            "cold path: nothing cached → fetch all 2 layers"
        );
        assert_eq!(
            cas.write_count.get(),
            2,
            "cold path: 2 write-backs (one per fetched layer)"
        );
    }

    #[test]
    fn invalid_plan_empty_lease_id() {
        let mut plan = sample_plan_unit();
        plan.lease_id = "  ".to_string();
        assert!(
            matches!(
                hydrate(&AllCachedFake, &plan),
                Err(BootError::InvalidPlan { .. })
            ),
            "empty lease_id must produce InvalidPlan"
        );
    }

    #[test]
    fn invalid_plan_empty_content_key() {
        let mut plan = sample_plan_unit();
        plan.toolchain_layers.push(ToolchainLayer {
            content_key: "".to_string(),
            size_bytes: 0,
        });
        assert!(
            matches!(
                hydrate(&AllCachedFake, &plan),
                Err(BootError::InvalidPlan { .. })
            ),
            "empty content_key must produce InvalidPlan"
        );
    }

    #[test]
    fn substrate_down_zero_writes() {
        struct DownFake;
        impl BootCas for DownFake {
            fn is_cached(&self, _key: &str) -> bool {
                false
            }
            fn fetch_layer(&self, _key: &str) -> Result<Option<Vec<u8>>, BootError> {
                Err(BootError::SubstrateDown {
                    substrate: "CAS".to_string(),
                    reason: "test: CAS down".to_string(),
                })
            }
            fn write_layer(&self, _key: &str, _data: &[u8]) -> Result<(), BootError> {
                panic!(
                    "write_layer must NOT be called after a fetch failure (poisoned write guard)"
                )
            }
        }

        let plan = sample_plan_unit();
        let result = hydrate(&DownFake, &plan);
        assert!(
            matches!(result, Err(BootError::SubstrateDown { .. })),
            "CAS down must produce SubstrateDown error; got: {result:?}"
        );
        // The panic in write_layer would fire here if the ordering is wrong.
    }
}
