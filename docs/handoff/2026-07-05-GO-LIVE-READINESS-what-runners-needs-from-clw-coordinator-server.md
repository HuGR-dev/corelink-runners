# GO-LIVE READINESS → clw owner + coordinator + Server TL — the exact critical path to launch the runner (my side is DONE)

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-05
> One consolidated ask. My entire half is built, merged, and deployed-where-safe. Go-live is gated on **three
> external items** (one per addressee) + my final smoke. Each item below: what I need, why it gates launch, and
> exactly what I do the moment it lands.

## Where my side stands (nothing here blocks launch)
- Data plane + CF-native spawn path + **re-drive reconciler** (deployed + live-validated: CI self-fed, no manual drain).
- **R1 vCPU ceiling** armed + durable (Neon PgLedger on CF-fabricd) + enforcement wiring complete (`max_vcpu_h` in
  the strict `IntrospectBody` + shared conformance vector, byte-parity confirmed with Server, **#289 merged**).
- **#283 cf-multitenant Worker half** built + merged (drops `owner_tenant`, authorizes-before-JIT, injects the
  server-derived tenant, `max_concurrency` gate).
- **env-0** cred-ticket broker built + merged (#287), inert until armed.
- Billing usage-push + O7 isolation (metadata-unreachable, ADR-0009) closed.

---

## GATE 1 → clw owner: **cut a clw release ≥ 0.1.4 containing PR #165** (unblocks env-0 — a pre-launch gate)
**Why it gates launch:** the owner's no-waiver ruling (2026-07-04) makes env-0 pre-launch — the untrusted runner
must NOT carry the CAS PAT in its env. env-0 is built + ready, but the deployed runner image pins clw **v0.1.1**,
and the cred-ticket redemption (`CredentialSource`/`redeem_cred_ticket`, PR #165, commit `0710168`) is **UNRELEASED**
(absent from tags v0.1.1/0.1.2/0.1.3; HEAD only, untagged 0.1.4). Arming env-0 on v0.1.1 cold-breaks cache-warm.
**Need:** a `v0.1.4`(or later) tag at/after `0710168`, published to `HumanGuardrail/clw-releases` with the signed
`SHA256SUMS` (minisign `4B57B8B54A0E396D`) — confirm the tag + the `x86_64-unknown-linux-gnu` sha256.
**I then:** bump `deploy/runner/Dockerfile` (`CLW_VERSION`+`CLW_SHA256`) → rebuild+push the runner image → update the
pinned digest in `deploy/cloudflare/wrangler.jsonc` → arm env-0 (`SPAWN_WORKER_PUBLIC_URL`) → exit test.
(Full detail: `docs/handoff/2026-07-05-ASK-to-clw-owner-cut-a-release-with-165-so-env0-can-arm.md`.)

## GATE 2 → coordinator: **the step-3 go-live ping** (deploy the mint gargalo)
**Why it gates launch:** #283 correctly fails-open to cold until the server mint is live; the Server has deployed the
mint half + handed you the GO (`2026-07-05-GO-STEP3-from-server-tl-map-is-LIVE…`). The atomic step-3 window is yours.
**Need:** the ping to open the deploy window.
**I then (same window):** deploy the CF Worker half (`wrangler deploy --containers-rollout=none`) + arm
`FABRIC_GITHUB_MINT_TOKEN` on the fabricd broker + smoke. A webhook from an allowlisted repo → runner minted under
its real tenant; an off-allowlist repo → 403, no spawn.

## GATE 3 → Server TL: **confirm a `runners_entitlement` row exists for the launch tenant** (else fail-closed wall-off)
**Why it gates launch:** you wired the introspect to emit `max_vcpu_h`, and the asymmetry is **absent `max_vcpu_h`
⇒ wall-off (fail-closed)**. So a launch tenant with NO `runners_entitlement` row cannot acquire — the ceiling walls
it off. **Need:** confirm the D1 `runners_entitlement` table has a row for the launch tenant
**`d863fafb-17c3-4ec3-92f6-b5a85c27d7bd`** with a sane `max_concurrency` + `max_vcpu_h` (the values you intend to
enforce at launch). If it's absent/0, the tenant either walls-off (vCPU) or hits no-plan (concurrency) — surface the
intended launch numbers.
**I then:** nothing — this is the value my armed ceiling enforces; I just need it populated so the smoke passes.

---

## The go-live sequence (once the 3 land)
1. **GATE 1 + GATE 3** can land in parallel with GATE 2. GATE 1 (clw release) is the long pole (a release cut).
2. On **GATE 2** ping → I deploy #283 + arm the mint token + smoke the tenant-correct spawn.
3. On **GATE 1** release → I re-pin the runner image + arm env-0 + exit-test (no PAT in the lease env, cache still
   hydrates).
4. With **GATE 3** confirmed → the smoke also exercises the enforced ceiling.
5. **Launch** = all four green: tenant-correct mint (2) + PAT-not-in-env (1) + ceiling enforced (3) + E2E smoke (4).

**Critical path = GATE 1 (clw release).** GATES 2 and 3 are ready-to-signal. Ping me per gate as each lands; I turn
each around in-window and report the smoke result.

— corelink-runners TL
