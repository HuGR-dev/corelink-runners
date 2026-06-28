# DESIGN-HANDOFF — runner-acquire authorization (RunnerScope tenant-binding + allowlist on /v1/leases) + materialize RAII

> **From:** autonomous audit loop (round 5) · **To:** owner / M2 control-plane wave · **Date:** 2026-06-28
> **Why a handoff, not a patch:** the core fix (RunnerScope→tenant binding) needs a **policy model that does not exist yet** (tenant→permitted owners/orgs), and the exposure is **latent** — it requires two tenants sharing one GitHub App installation, i.e. the M2 multi-tenant control plane (not yet live; single-tenant dogfood today). These are M2-GA preconditions, scoped here so the wave lands them.

## #2 (medium, latent) — RunnerScope is never bound to the authenticated tenant
`runner_scope_from_dto` (`leases.rs`) is a pure 1:1 structural map (owner/repo/org copied verbatim from the caller DTO). The authenticated `TenantId` gates cap/rate/ledger admission but is **never consulted to constrain the RunnerScope** handed to `mint_jit_offloaded` → the broker. With a single shared GitHub App `installation_id`, a tenant could mint a JIT runner config for ANY repo/org the install can reach.
- **Why latent:** today this is a single-tenant system (ADR-0002/0007 — HuGR's own repos, one bootstrap tenant). Cross-tenant exploitation needs a 2nd onboarded tenant sharing the install — an M2 state.
- **Fix (M2):** bind the scope to the tenant — pass `&tenant` into `runner_scope_from_dto` and validate `runner.target` against a per-tenant allow-mapping (tenant→permitted owners/orgs); reject otherwise. This belongs with the M2 identity model (per-tenant GitHub App install OR a tenant↔repo policy table).

## #3 (medium, latent) — repo_allowlist enforced on the webhook path but NOT on /v1/leases
`state.cfg.repo_allowlist` gates the autoscaler webhook path (`webhook.rs:514-522`) but the direct acquire path (`leases.rs` runner branch) has no equivalent gate.
- **Cheap interim (operator control, valuable even single-tenant):** mirror the webhook's allowlist check in the runner branch of the acquire/finalize path (or inside `runner_scope_from_dto`) — reject a target not in `repo_allowlist` with the frozen `Invalid`/404. This is a small, self-contained change; it was deferred from the round-5 hardening PR only to keep that PR to clearly-exploitable items, and because it pairs naturally with #2's tenant-binding. **Recommend landing it early in the M2 wave (or standalone) — it's low-risk defense-in-depth on the primary front door.**

## #4 (medium, low practical risk) — partial materialize has no rollback / no RAII teardown
`materialize_sparse` (`materialize/mod.rs`) places files one-by-one; a `place_file` failure returns `Err` with no rollback of already-written in-fence files, and `RunningContainer` has no `Drop`.
- **Why low practical risk:** the container is ephemeral and the fail-closed acquire failure tears the whole box down (the partial files die with it); there is no reuse of a partially-materialized container.
- **Fix (type-hygiene):** make the error carry the container handle (`MaterializeError::Box { container, source }`) so the caller is forced to handle the partial container explicitly, and/or `#[must_use]` the materialize entrypoint. A `rollback_materialize` helper (remove the in-fence files placed so far) is the belt-and-suspenders option.

## #11 (low, not a defect) — ClwRunSpec upstream validation
Injection is already held by `shell_join` quoting (Inv-4 HOLDS). Optional moat-hardening: a `validate_clw_run_spec` restricting `snapshot_name`/`snapshot_path`/`hydrate_dest` to a shell-inert charset (`^/[A-Za-z0-9._/-]+$`, mirroring `validate_tmp_root`). No exploitable gap; defense-in-depth only.

All tracked in `2026-06-28-audit-loop-round-5.md`. None is a live exploit today; #2/#3 are M2-GA preconditions.
