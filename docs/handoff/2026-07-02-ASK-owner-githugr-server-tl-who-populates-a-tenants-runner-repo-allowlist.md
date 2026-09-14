# ASK → owner + githugr TL + Server TL — WHO populates a tenant's runner `repo_allowlist`? (the only thing between C1's fail-closed gate and CoreLink-authed runner leases)

> **FROM:** corelink-runners TL · **TO:** owner, githugr TL, CoreLink Server TL · **Relay:** owner · **DATE:** 2026-07-02

## Where we are
- **C1 (merged):** a runner acquire's target repo/org must be on the caller tenant's `repo_allowlist`, FAIL-CLOSED. Static tenants use `FABRIC_RUNNER_REPO_ALLOWLIST`; the CoreLink-auth path is fail-closed (no runner lease) until the entitlement carries the allowlist.
- **C4 (Server TL, 2026-07-02):** flip `FABRIC_AUTH_BACKEND=corelink` NOW for auth + `max_concurrency` + `max_vcpu_h` (all live, frozen, auto-provisioned at signup). But the entitlement does **NOT** carry `repo_allowlist` — and the Server TL (correctly) won't ship an empty field pretending it's the feature.
- **The blocker is DATA, not schema:** CoreLink can't know a tenant's repos at signup. **That data is githugr's** (githugr knows which repos/orgs a user owns/can run on).

## The one decision I need (3-way)
**How does a tenant's allowed repo/org set reach the runner-lease gate?** Pick the population source:
1. **githugr PUSHES it** to a CoreLink internal endpoint (mirrors how githugr already pushes identity) → Server TL adds `repo_allowlist` as an additive frozen field on `runners_entitlement` → I read it through (already fail-closed-tolerant). *(Server TL's guess; my lean too — it reuses the existing identity-push seam.)*
2. **Operator/admin sets it** per tenant (manual; fine for a handful of dogfood tenants, doesn't scale).
3. **Derived at acquire-time** from the caller's githugr token (the fabric resolves the caller's repos live per-acquire) — most dynamic, but puts githugr-token resolution on the hot admission path.

## What each unlocks
- Until decided: **CoreLink-authed tenants can auth + run checks, but NO runner leases** (C1 fail-closed) — the dogfood tenant `d863fafb` keeps running the static path, which is fine. So this does **not** block the C4 flip for auth+cap+ceiling.
- Once decided: Server TL adds the additive field (small WP), githugr wires the push (if option 1), I read it through — and CoreLink-authed multi-tenant runner leases go live.

## Ask
- **Owner + githugr TL:** which population source (my + Server-TL lean: **option 1, githugr-push**)?
- Once picked, Server TL adds the field, githugr wires the push, I consume it. Small on all three sides.

This is NOT blocking the C4 flip (auth+cap+ceiling is ready); it's the path to get runner-repo-gating off static for real multi-tenant.

— corelink-runners TL
