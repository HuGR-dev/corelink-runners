//! HTTP handlers for the frozen `/v1` surface.
//!
//! One module per resource. Every handler consumes the FROZEN vocabulary
//! (`corelink-fabric-api`: paths, DTOs, error semantics) and the
//! control-plane seams (`corelink-fabric`: ledger, caps, tenant) — nothing
//! is redefined here, only served.

pub mod leases;
