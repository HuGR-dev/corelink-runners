//! # corelink-cloud-engine
//!
//! The **CloudSandboxEngine**: an implementation of the runner's frozen
//! [`Engine`](corelink_runner::isolation::Engine) seam against a managed
//! microVM/sandbox provider, so the fabric can run untrusted, per-job-isolated
//! compute without operating metal or building Firecracker.
//!
//! The backends are [`NorthflankEngine`], targeting Northflank's Job-run API —
//! the pay-per-use, scale-to-zero provider the pricing model is anchored on
//! (`docs/product/pricing.md`) — and [`CloudflareEngine`] (ADR-0008), targeting a
//! CoreLink spawn-Worker (a Cloudflare Worker + Durable Object that spawns a
//! Cloudflare Container) over the same [`HttpTransport`](http::HttpTransport)
//! seam, DEFAULT-OFF (selected by env, absent ⇒ not wired). Because the engine is
//! the [`Engine`] trait, swapping the provider — or moving to a self-hosted
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

pub mod cloudflare;
pub mod http;
pub mod northflank;

pub use cloudflare::{
    CLOUDFLARE_EXPIRY_MS_ENV, CLOUDFLARE_RUNNER_LABELS_ENV, CLOUDFLARE_RUNNER_STORAGE_MB_ENV,
    CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV, CLOUDFLARE_SPAWN_WORKER_URL_ENV, CloudflareConfig,
    CloudflareEngine, DEFAULT_EXPIRY_MS, DEFAULT_RUNNER_STORAGE_MB,
};
pub use http::{
    DEFAULT_HTTP_TIMEOUT, HTTP_TIMEOUT_ENV, HttpRequest, HttpResponse, HttpTransport, Method,
    UreqTransport, timeout_from_env_with,
};
pub use northflank::{
    NorthflankConfig, NorthflankEngine, ProviderCapacityError, RUNNER_EPHEMERAL_STORAGE_FLOOR_MB,
    RunnerDiskStatus,
};
