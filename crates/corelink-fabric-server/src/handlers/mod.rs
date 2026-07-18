//! HTTP handlers for the frozen `/v1` surface.
//!
//! One module per resource. Every handler consumes the FROZEN vocabulary
//! (`corelink-fabric-api`: paths, DTOs, error semantics) and the
//! control-plane / capture seams (`corelink-fabric`: ledger, caps, tenant;
//! `corelink_runner::envelope`: the §13 hook mechanism) — nothing is
//! redefined here, only served. Routes are assembled in [`crate::app`] from
//! the FROZEN path constants (`corelink_fabric_api::paths`).

pub mod admin;
pub mod agent_exec;
pub mod cas_cred;
pub mod close;
pub mod enforcement;
pub mod envelope;
pub mod exec_handler;
pub mod lease_list;
pub mod leases;
pub mod metrics;
pub mod occupancy;
pub mod queue;
pub mod status;
pub mod test_mint;
pub mod usage;
pub mod usage_history;
pub mod webhook;
