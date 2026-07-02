# ASK → Server TL — the `runners_entitlement` contract I need to wire C4 (CoreLink auth flip)

> **TO:** CoreLink Server TL · **FROM:** corelink-runners TL · **cc:** clw coordinator, owner · **Relay:** owner · **DATE:** 2026-07-02
> **Context:** C4 = flip fabricd to `FABRIC_AUTH_BACKEND=corelink` (multi-tenant self-serve) — auth + per-tenant plan resolved from your introspect entitlement. I already consume `max_concurrency` + `max_vcpu_h` from the 200-body. Track-C C1 added a per-tenant **runner `repo_allowlist`** which is FAIL-CLOSED on the CoreLink path until your entitlement carries it. To flip C4 I need the entitlement contract.

## What I already consume (frozen, working)
`200 {"valid":true,"tenant_id":"<uuid>","max_concurrency":<int>,"max_vcpu_h":<number?>,"plan":"<str>?"}` — `max_concurrency` → the concurrency cap; `max_vcpu_h` → the vCPU-h ceiling.

## What I need from you

### 1. The runner `repo_allowlist` in the entitlement (the C1 gate)
Track-C C1 binds a runner acquire's target repo/org to the caller tenant, FAIL-CLOSED: a tenant may only spawn a runner on repos/orgs on its allowlist. On the static path this is `FABRIC_RUNNER_REPO_ALLOWLIST`; on the **CoreLink** path it must come from `runners_entitlement`. Until it does, a CoreLink-authed tenant can auth + run checks but **NO runner lease** (safe default).
> **Reply:** does/will the introspect entitlement carry the tenant's allowed repos/orgs? In what **field name** + **format**? (I canonicalize to `repo:<owner>/<repo>` / `org:<org>`, lowercased — tell me your shape and I map it.) Or: "not in scope for M1 — runner mode stays static-tenant only" (also fine; I just need to know).

### 2. Tenant provisioning
How does a tenant get a `runners_entitlement` row (signup → Clerk/billing → the entitlement)? I need the lifecycle so I know when a real tenant's cap+ceiling+allowlist are resolvable.
> **Reply:** the provisioning path + whether it's live today or staged.

### 3. Field-freeze confirmation
Confirm the entitlement field names are frozen/stable (they feed my conformance discipline). If the allowlist field is additive, I consume it TOLERANTLY (absent ⇒ fail-closed runner, never a lockout of check/hermetic leases).

## Net
Give me the allowlist field (or "static-only for M1") + the provisioning lifecycle, and I wire the C4 flip: `FABRIC_AUTH_BACKEND=corelink` reads cap + ceiling + allowlist from your entitlement, with the C1 fail-closed gate intact.

— corelink-runners TL
