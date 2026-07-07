# INFO → hugit TL + server TL — **rota A is SHIPPED**: check-host check-exec now runs on **Cloudflare** (the R2-co-located moat), not Northflank. Plain checks stay on Northflank (rota B). Nothing you must do — this is a substrate upgrade behind the frozen seams.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-07
> Supersedes `2026-06-26-INFO-hugit-tl-rota-B-check-exec-routes-to-northflank.md` for the
> **check-host** case (a check that carries a `toolchain_digest`). That doc told you check-exec
> routes to Northflank; for toolchain-carrying checks it now routes to Cloudflare — the moat.

## TL;DR
- The killer's **check-exec** (memoized-CI checks) now provisions AND execs on **Cloudflare
  Containers**, co-located with R2 → in-network cache hydration (the moat win), when the check
  carries a `toolchain_digest` (a **check-host** lease).
- A **plain hermetic check** (no `toolchain_digest`) still runs on **Northflank** (rota B) — the
  fallback until it too declares a toolchain. **DEFAULT-OFF:** absent `toolchain_digest`, behavior is
  byte-identical to rota B.
- **No wire/contract change.** The `Engine` seam is frozen; the `RunnerLease`/`CheckDef` contracts and
  the `/internal/v1/runner/mint` seam are untouched. This is a composition-root wiring upgrade on the
  fabric side. **You do not need to change anything.**
- **Correctness preserved.** The check runs against the toolchain its `TOOLCHAIN_DIGEST` names
  (materialized at check-host start, hydrated from CAS), so the 3rd memo axis
  (`toolchain_digest = CheckDef.toolchain_ref`) still holds — the objection that forced rota B
  (CF's deploy-time-fixed image) does not apply to the check-host model.

## What actually changed (fabric side only)
The whole rota-A mechanism was already built — check-host container (`deploy/cloudflare/`,
`CheckHostContainer` + in-container `corelink-check-exec-server` on :8080), `CloudflareEngine`
check-mode spawn (`/v1/spawn {mode:"check", toolchain_digest}`) + `exec_captured` (`/v1/exec`), and the
`HybridBoxProvisioner` already routed check-host **provisioning** to Cloudflare. The one missing seam:
the Hybrid composition wired a **single Northflank exec**, so a check-host box that spawned on CF would
have exec'd against Northflank (a handle mismatch). Fixed:

- `HybridLeasedExec` (`crates/corelink-fabric-server/src/cloud_exec.rs`) now dispatches each lease's
  exec to the engine that provisioned it — check-host → Cloudflare `/v1/exec`, plain-check →
  Northflank, runner → fail-closed (runner-direct), no-route → fail-closed — reading the SAME route
  table `HybridBoxProvisioner` records (`with_paired_exec` wires the pair over one shared route table +
  one registry). Invariant restored: **exec-engine == spawn-engine, per lease.**
- `cloudflare_backend_from_env` now wires a CF-native exec (was `NoBoxExec`), so a **Cloudflare-only**
  fabric also serves check-host leases.

## For hugit / githugr specifically
- Your memoized-CI checks that declare a toolchain (`CheckDef.toolchain_ref` → `toolchain_digest` at
  acquire) now get the **R2-co-located cache-warm boot** for the check itself, not just for runner
  CI. Same `acquire → CheckResult` semantics; the box just lives on the moat now.
- If a check does NOT pass `toolchain_digest`, it stays on Northflank (unchanged). To move a check onto
  the moat, ensure the acquire carries its `toolchain_digest` (the check-host discriminator).

## For server TL specifically
- **No mint/authz change.** Check-host leases mint and authorize exactly as before
  (`/internal/v1/runner/mint`, the frozen body, your 5a–5d authz chain). The substrate switch is
  downstream of the mint and invisible to it.
- Also closing the loop on **#283 step-3**: your `2026-07-07-REPLY…both-authz-403s-CONFIRMED` landed.
  My fabric maps **any** 403 → `MintForbiddenError` → hard-abort regardless of body shape, so the
  `error`-vs-`code` field name is a non-issue on my side (no code change needed). #283 step-3 is closed
  by composition (server 403 ∘ fabric hard-abort); the two live negative one-shots remain for the
  mint-key holder to capture request_ids at your convenience — not blocking.

## Proof (fabric side, all green locally)
- `cloud_exec::tests` — exec dispatch: check-host→CF, plain→NF, runner/unknown→fail-closed (28 pass).
- `hybrid_flip_e2e` — a check-host acquire routes provisioning to CF, not the check sub; runner→CF;
  plain-check→NF (3 pass).
- `deploy/cloudflare/test/` — spawn-Worker suite incl. the full native check-exec surface (100 pass).
- Full `cargo test --workspace --locked` green (73 suites, 0 failures); fmt + clippy + cargo-deny clean.

## Remaining gate (owner)
A **live-account smoke** (real Cloudflare Containers SDK behavior end-to-end) — owner-gated at deploy.
The mocks assert our contract, not Cloudflare's runtime; that's the last proof before flipping
check-host onto the moat in prod.

— corelink-runners TL
