# DIRECTIVE (owner-apply) — arm the vCPU-h compute ceiling: `FABRIC_RUNNER_VCPU=4`

> **OWNER:** corelink-runners TL · **DATE:** 2026-07-02 · **STATUS:** code DONE + gate-green; this is the deploy-time arm, value **decided**.

## The value — decided, not guessed
`FABRIC_RUNNER_VCPU=4`. It **must equal** the real runner-container size so the vCPU·ms accounting is honest. The substrate provisions **`standard-4` = 4 vCPU / 12 GiB** per runner/check container (`deploy/cloudflare/wrangler.jsonc:58-70`). Any other value would over- or under-bill the `max_vcpu_h` ceiling. → **4**.

## The exact arm (fabricd env, all three together)
```
FABRIC_RUNNER_VCPU=4          # arms the ComputeGate; must match standard-4
FABRIC_LEDGER_BACKEND=pg      # REQUIRED — accounting-on fails-closed at boot without a
                              # cross-instance ledger (server.rs:757). Ceiling must never
                              # run on the in-memory ledger (a restart would zero accrual).
FABRIC_AUTH_BACKEND=corelink  # item #1 — the per-tenant max_vcpu_h entitlement rides the
                              # CoreLink introspect; the ceiling is inert without it.
```

## Why it's fail-closed-safe
- **Default-off:** absent `FABRIC_RUNNER_VCPU` (or `=0`) ⇒ the ceiling wall is DORMANT, byte-identical to today (`acquire` passes no `ComputeGate`). Nothing changes until you set it.
- **No silent mis-arm:** an unparseable value is a HARD boot error (`runner_vcpu_unparseable_is_a_hard_boot_error`), and arming without `pg` is a HARD boot error. You cannot half-arm it.
- Enforcement is proven end-to-end by `corelink_flip_e2e.rs` (`max_vcpu_h × 3_600_000` = `ceiling_vcpu_ms`; acquire past the ceiling → reject).

## Coupling
This rides the **same fabricd deploy as item #1** (auth flip). #1 supplies the per-tenant `max_vcpu_h`; #2 supplies the per-runner vCPU size that turns wall-time into vCPU·ms. Arm both in one deploy, or neither.

**Nothing to build. This is the switch + the Postgres prerequisite. Apply on the next fabricd deploy.**
