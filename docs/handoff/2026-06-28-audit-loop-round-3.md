# Autonomous audit loop — Round 3 (2026-06-28, ~05:20 local)

8 fresh prove-or-break lenses (3 Opus + 5 Sonnet) → adversarial verify. **2 confirmed (1 medium, 1 low), 0 refuted.** Raised 2 — the codebase is **converging** (rounds 1→2→3 confirmed: 3 → 10 → 2; high/critical: 0 → 2 → 0).

## Confirmed + disposition
| # | Sev | Finding | Disposition |
|---|-----|---------|-------------|
| 1 | **low→KEY** | **CoreLinkPlanStore has no bounded retry** (`corelink_plans.rs` `plan_of_resolving`). #204 added the cold-start retry to the *token* store (`tenant_of`) but NOT the *plan* store. The acquire path does BOTH introspects; a cold-egress blip the auth call survived (3 attempts) killed the plan call (1 attempt) → 503. **This is the githugr endpoint-specific `/v1/leases` 503** (`/readyz` only auths → recovered; `/v1/leases` also resolves the plan → 503). | **FIXED (this PR)** — mirrored `tenant_of`'s bounded retry into `plan_of_resolving` (retry only transient transport-err/503; authoritative 200/401 immediate; bounded to `INTROSPECT_ATTEMPTS`, shared const). +2 regressions (503→200 recovers; 401 not retried). **Completes the cost-killer's last gate.** |
| 2 | **med** | **RunnerTargetDto enum missing `#[serde(deny_unknown_fields)]`** (`dto.rs:70`) — the lone wire DTO violating the module's own "every body deny_unknown_fields" invariant; unknown fields in the nested `repo`/`org` payload are silently dropped, not rejected. (Practical impact low — Rust discards them — but the documented contract is broken.) | **RELAYED** (`2026-06-28-RELAY-runnertargetdto-deny-unknown-fields.md`) — `dto.rs` is the **frozen wire-contract crate**; per the inviolable wire-contract law I do NOT edit it unilaterally. Owner / hugit-techlead coordinate the (both-sides) change. |

## Note on #1 (the cost-killer)
The earlier #204 was a *partial* fix (it covered `tenant_of` only). This round found the missing half (`plan_of_resolving`), which is the actual endpoint-specific behavior githugr's differential probe isolated. With both introspect paths retried, a cold acquire on `/v1/leases` should now recover to `200 AcquireResponse`. **Live confirmation still needs the fabricd redeploy** (owner-gated; the deploy stalled overnight on slow Docker) — code-complete + tested here.

## Gate
`fmt` · `clippy --workspace -D warnings` · `cargo test --workspace` green.
