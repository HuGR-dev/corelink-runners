# Autonomous audit loop — Round 9 (OPS / SUPPLY-CHAIN / DEPLOY, 2026-06-28, ~08:45 local)

The fresh pre-GA angle the code-logic rounds didn't cover: cargo-audit/deny advisories, the runner-image Dockerfile/entrypoint, the CI workflows, the CF Worker deploy/secrets, a committed-secret scan, env/secret handling, and lockfile/license hygiene. 3 Opus + 5 Sonnet → adversarial verify. **12 confirmed (1 medium, 11 low), 3 refuted.**

**Headline: supply-chain advisories are CLEAN** — `cargo deny check` → *advisories ok, bans ok, licenses ok, sources ok* (no RUSTSEC advisory, no yanked crate, crates.io-only sources enforced). **No committed secret** was found (the scan hits were variable names / test fixtures / redacted Debug / obviously-fake examples). Refuted (correctly): DCO-on-fork-PRs (not real), PINNED-absent-from-wrangler (guarded in code), and "live Stripe Price IDs committed" (price IDs are not secret).

## Confirmed + disposition
| # | Sev | Finding | Disposition |
|---|-----|---------|-------------|
| 1 | **med** | rustup-init bootstrap is `curl\|sh` **unpinned** (`deploy/runner/Dockerfile`) while the runner tarball + clw are sha256-verified; the comment **falsely** claimed verification — an X4 supply-chain-floor gap. | **PARTIAL FIX + RELAY** — the misleading comment is corrected in code now (states the truth); the pin needs a **human-verified** rustup-init SHA (`RUSTUP_INIT_SHA256` build-arg, mirroring `CLW_SHA256`) → `2026-06-28-RELAY-rustup-init-pin.md`. Not fabricating a checksum. |
| 2 | low | stale `cargo-deny` skip `wit-bindgen@0.57.1` (single version → no duplicate; masks future dupes). | **FIXED (this PR)** — removed the skip + corrected the comment; `cargo deny check` re-verified green. |
| 3 | low | cargo-audit unverifiable locally (tool not installed). | **NOT-A-DEFECT** — env limitation; cross-checked clean via `cargo deny` (no `[advisories]` suppression, all crate versions current/non-advisory). |
| 4 | low | `pull_request` CI on the persistent self-hosted builder. | **NOT-A-LIVE-ISSUE** — the repo is **private** (CLAUDE.md/ROADMAP/RUNBOOK confirm); no anonymous fork PRs. Standard caution for if it ever goes public. |
| 5 | low | `.dev.vars` not in the CF `.gitignore` (accidental local-secret-commit risk). | **FIXED (this PR)** — added `.dev.vars*` to `deploy/cloudflare/.gitignore`. (Nothing is committed today.) |
| 6 | low | `workflow_dispatch` input interpolated into a shell `run` step (injection); + mutable action tags (`@v4` not `@sha`). | **NOTED** — workflow_dispatch is maintainer-only (low); action-SHA-pinning is a CI-hardening follow-up. Tracked. |
| 7 | low | the fabricd proxy Worker forwards `/internal/v1/admin/tenants` + `/occupancy`. | **NOTED** — admin onboard is key-gated + default-off (404, hardened r7); occupancy is internal observability. Confirm the proxy doesn't expose `/internal/*` unauthenticated as a CI-hardening follow-up. |
| 8 | low | `AppState::new()` defaults to a committed DEV signing key. | **NOTED** — the composition root (`build_app_and_state`) requires the real key; `new()`/`bare_state` are the test path. Confirm no prod path reaches the default (round-7 public-API lens found none reachable). |
| 9–12 | low | BILLING_REGION wrong-length→None; northflank-service.json template inline-env; package-lock drift (cloudflare vs cloudflare-fabricd). | **NOTED** — config-robustness / template / lockfile-hygiene; non-security, tracked for the deploy-polish wave. |

## Status
Supply-chain + secrets are clean; the one real gap (rustup pin) is comment-corrected now + relayed for the human-verified SHA. The cheap hygiene wins (deny skip, `.dev.vars`) are fixed. The remaining lows are CI/deploy-hardening follow-ups (action-SHA-pinning, proxy-internal-path confirm, action injection), all noted — none a live exploit.
