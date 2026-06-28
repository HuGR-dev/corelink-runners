# Autonomous audit loop — Round 5 (2026-06-28, ~07:00 local)

8 lenses on the **least-audited crates** (check-exec-server, runner-broker, cloud-engine clients, materialize internals, cas-mint/inject, clw-drive, contracts, envelope-ack). 3 Opus + 5 Sonnet → adversarial verify. **12 confirmed, 4 refuted** — but 4 of the "confirmed low" are actually **INVARIANT-HOLDS proofs** (the check-exec-server lens confirmed INV-1/2/4/5 hold: direct argv exec / SIGKILL-group→null / server-side cwd / container-boundary gate), not defects.

## Real findings + disposition
| # | Sev | Finding | Disposition |
|---|-----|---------|-------------|
| 1 | **high** | **Reaper unregisters the §13 hook BEFORE flushing** — `flush_partial_envelope` ran after `forget_lease`, so TIER-1 found no hook → in-process partial metrics silently dropped to tier-3 zero on expiry/crash (`reaper.rs`). | **FIXED (this PR)** — flush BEFORE forget in both `reap_once` + `surface_crashes`; strengthened the direct flush test to assert TIER-1 non-zero metrics. |
| 2 | med | **RunnerScope taken verbatim from the request, never bound to the authenticated tenant** — cross-tenant runner mint / scope escalation (`leases.rs`). | **DESIGN-HANDOFF** (`2026-06-28-DESIGN-runner-scope-tenant-binding.md`) — **latent**: needs two tenants sharing one GitHub App install (multi-tenant M2, not yet live). The policy model (tenant→permitted owners) is an M2-GA precondition. |
| 3 | med | `repo_allowlist` (defense-in-depth) enforced on the webhook path but **absent on `/v1/leases`** (`leases.rs`). | **FIXED (PR round5-hardening)** — mirror the webhook's allowlist gate on the acquire path. |
| 4 | med | Partial `materialize` leaves in-fence files with no rollback / no RAII teardown (`materialize/mod.rs`). | **DESIGN-HANDOFF** (folded into the runner-scope doc's "type-hygiene" note) — **low practical risk**: the container is ephemeral and torn down on the fail-closed acquire failure; the fix is a `#[must_use]` / error-carries-container type refactor (moat is default-off). |
| 5 | med | `AttestationChain` sig-preimage has **no cross-repo conformance vector** (unlike `result_binding_v2`) (`conformance/`). | **RELAYED** (`2026-06-28-RELAY-attestation-chain-conformance-vector.md`) — a conformance vector is added cross-repo (fabric + the client verifier), never unilaterally. |
| 10 | low | Northflank `run_id` from the provider response interpolated into the poll URL **without charset validation** (cloudflare's `parse_handle` validates; northflank's `parse_id` doesn't) (`northflank.rs`). | **FIXED (PR round5-hardening)** — reject non-`[A-Za-z0-9_-]`, mirroring `cloudflare::parse_handle`. |
| 11 | low | `ClwRunSpec` fields have no upstream charset validation (shell-injection is held only by `shell_join` quoting — Inv-4 HOLDS, but defense-in-depth) (`clw`). | **FIXED (PR round5-hardening)** — `validate_clw_run_spec` (shell-inert charset), mirroring `validate_tmp_root`. |
| 12 | low | `RunnerState` terminal variants (released/expired/crashed) have no shared cross-repo conformance vector (`conformance/`). | **RELAYED** (folded into #5's relay) — added cross-repo. |

## Refuted / proven-holds
check-exec-server INV-1/2/4/5 (no shell-injection, timeout→group-kill→null, server-side cwd, container-boundary gate) + INV-3 (8 MiB output cap) all HOLD. clw-drive Inv-1/2/3 (exit-transparency at the type level, unconditional spawn_blocking, hydrate fail-closed) HOLD. The fabric's least-audited core is sound.

## Status
The **high** (forensic-metrics loss) is fixed here. The cheap defense-in-depth lows + the allowlist medium land in the round-5-hardening PR. Latent/architectural (runner-scope tenant-binding, materialize RAII) + conformance-vector gaps (attestation-chain, RunnerState terminals) are handed off / relayed — all coordinated or M2-gated, none silent debt.
