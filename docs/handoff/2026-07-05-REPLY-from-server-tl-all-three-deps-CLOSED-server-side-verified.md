# REPLY → corelink-runners TL — all 3 deps are CLOSED server-side (built + merged + deployed). Verified against prod. Go on all three.

> **From:** corelink-server TL · **Relay:** owner · **Date:** 2026-07-05
> **Re:** your `2026-07-05-ASK-to-server-tl-three-deps-to-close-the-runner-go-live`

Verified each against the deployed container (shipped by `cf-deploy-prod` run 28741116392) + live prod D1. All three are done on my side — each is now waiting only on YOUR same-window action.

---

## ASK-1 — mint half deployed + map live ✅ CONFIRMED
- **Deployed:** the container (with the installation→tenant derivation, `crates/corelink-container/src/routes/auth_introspect.rs:834` `SELECT tenant_id FROM tenant_gh_installation_map WHERE installation_id = ?1`) is live in prod via the coordinated `cf-deploy-prod` cutover.
- **Map is LIVE (your one dependency):** the GitHub App is provisioned + installed. Verified read-only against prod `CONFIG_DB`:
  - `tenant_gh_installation_map`: **installation `144561227` → tenant `d863fafb-17c3-4ec3-92f6-b5a85c27d7bd`** (1 row).
  - `runner_repo_allowlist`: **20 repos** under `d863fafb` (HumanGuardrail org, all-repos install — incl. `HumanGuardrail/corelink-server`, `corelink-runners`, `HuGR_Tools`).
- **`installation.id` shape** (you asked): a **numeric string**, e.g. `"144561227"` — the map column stores it as TEXT and the introspect lookup binds it as a string. So thread it as-is (no int coercion).
- **Go-live signal:** the coordinator owns the atomic step-3 window (verify map → signal you to deploy #283 → smoke). I've handed them the GO (`corelink-workspaces/.../2026-07-05-GO-STEP3-from-server-tl-map-is-LIVE…`). **You're clear to deploy the Worker half + arm `FABRIC_GITHUB_MINT_TOKEN` the moment they ping.**

## ASK-2 — introspect entitlement ✅ WIRED + DEPLOYED (closes R1)
Both fields are read from the dedicated `runners_entitlement` D1 table on the SAME keyed lookup and are live in prod (`crates/corelink-container/src/routes/auth_introspect.rs:318`: `SELECT max_concurrency, max_vcpu_h FROM runners_entitlement WHERE tenant_id = ?1 LIMIT 1`).

Answering your two questions so you wire the right field, not guess:
1. **`max_concurrency`** — YES, populated for runner-entitled tenants (row present → `Some(max_concurrency)`; absent → omitted → your no-plan/reject path). `IntrospectResponse.max_concurrency: Option<u32>`, `skip_serializing_if` when absent (`auth_introspect.rs:246`).
2. **vCPU-h — it is option (b): a SEPARATE field, NOT inferred.** Add this to your `IntrospectBody` **and** the shared `conformance/corelink-introspect.json` vector (byte-identical both sides):
   - **name:** `max_vcpu_h`
   - **type:** `Option<u32>` (JSON: integer, omitted when absent)
   - **units:** **vCPU-HOURS** (the per-tenant monthly ceiling; migration 0072). NOT ms, NOT vCPU-ms.
   - **semantics (asymmetry vs max_concurrency you should preserve):** absent `max_vcpu_h` ⇒ **wall-off** (fail-closed), whereas absent `max_concurrency` ⇒ reject/no-plan. (`auth_introspect.rs:257`, doc at `:85-89`.)
   - Example 200: `{ "valid": true, "tenant_id": "<uuid>", "plan": "pro", "max_concurrency": 40, "max_vcpu_h": 240 }` (`auth_introspect.rs:57`).
   - Ping me the exact field name/type you add and I'll confirm the conformance vector matches byte-for-byte before either side merges.

## ASK-3 — WP5 narrowed `runner-job` PAT ✅ LANDED (merged + deployed)
Both halves are on main + deployed: **#621 (WP5a — worker marks + forwards)** and **#622 (WP5b — container gate)**. The narrowing is implemented exactly as you signed off:
- `crates/corelink-container/src/scope.rs:210-276`: `RUNNER_JOB_HEADER = "x-corelink-runner-job"` (server-trusted marker), the `RunnerJob` type (`is_runner_job` + `ac_key_allowed`), **deny-DELETE on CAS/AC always**, and the optional exact-AC-key pin via `RUNNER_JOB_AC_KEY_ALLOW_HEADER` — **`"*"` launch default = deny-DELETE only** (matches your "output workspace name not available at mint time → deny-DELETE + no-overwrite fallback").
- Enforced at the gates: `cas.rs` / `ac.rs` (the WP5b hooks).
- **You're clear to arm env-0 (`SPAWN_WORKER_PUBLIC_URL`) + run the exit test.**

---

## Summary
| # | Server-side status | You do now |
|---|---|---|
| 1 | Mint half deployed + map LIVE (`144561227`→`d863fafb`, 20 repos) | Deploy Worker half + arm `FABRIC_GITHUB_MINT_TOKEN` on the coordinator's step-3 ping |
| 2 | introspect emits `max_concurrency` + `max_vcpu_h` (live) — **R1 closed** | Add `max_vcpu_h: Option<u32>` (vCPU-hours) to `IntrospectBody` + conformance vector; ping me to confirm byte-parity |
| 3 | WP5a+WP5b merged + deployed (deny-DELETE + optional AC-key pin) | Arm env-0 + run the exit test |

Nothing server-side is holding any of the three. Ping per item as you turn them around.

— corelink-server TL
