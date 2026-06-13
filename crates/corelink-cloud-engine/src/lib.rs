//! # corelink-cloud-engine
//!
//! The **CloudSandboxEngine**: an implementation of the runner's frozen
//! [`Engine`](corelink_runner::isolation::Engine) seam against a managed
//! microVM/sandbox provider, so the fabric can run untrusted, per-job-isolated
//! compute without operating metal or building Firecracker.
//!
//! The first (and currently only) backend is [`NorthflankEngine`], targeting
//! Northflank's Job-run API — the pay-per-use, scale-to-zero provider the
//! pricing model is anchored on (`docs/product/pricing.md`). Because the engine
//! is the [`Engine`] trait, swapping the provider — or moving to a self-hosted
//! Firecracker backend later — never touches the fabric above it.
//!
//! ## Design discipline (mirrors the runner's `BoxExec` seam)
//! - The engine logic is generic over an [`HttpTransport`](http::HttpTransport);
//!   the real network lives in exactly one impl
//!   ([`UreqTransport`](http::UreqTransport)). `ureq` is named there and nowhere
//!   else, so the entire engine — and the entire test suite — is provider-API
//!   logic over a fake transport, with **zero account dependency**.
//! - The two security floors of [`DockerEngine`](corelink_runner::isolation) are
//!   preserved at `spawn`: the network-isolation floor and the X4 supply-chain
//!   (digest-pin) floor, both fail-closed before the provider is contacted.

pub mod http;
pub mod northflank;

pub use http::{
    DEFAULT_HTTP_TIMEOUT, HTTP_TIMEOUT_ENV, HttpRequest, HttpResponse, HttpTransport, Method,
    UreqTransport, timeout_from_env_with,
};
pub use northflank::{NorthflankConfig, NorthflankEngine};
