//! `corelink-cli` library surface.
//!
//! The `corelink` binary (`src/main.rs`) is a thin dispatcher over these
//! modules; they are exposed as a library so integration tests can drive the
//! real client/verify paths in-process against a real fabric, and so a future
//! embedder (an SDK, a CI plugin) can reuse them instead of re-transcribing.

pub mod binding;
pub mod client;
pub mod smoke;
