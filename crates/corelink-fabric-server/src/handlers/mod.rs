//! Fabric-side HTTP handlers over the frozen mechanisms. One module per
//! surface; routes are assembled in [`crate::app`] from the FROZEN path
//! constants (`corelink_fabric_api::paths`).

pub mod envelope;
