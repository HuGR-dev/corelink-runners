# DECISION (owner-ratified) — CF-native env-0: Option B. Launch with `CLW_TOKEN`-in-env accepted; port env-0 to the CF Worker LATER, in the namespace-scoping design pass.

> **Decided by:** owner (stakeholder) 2026-07-02, on the corelink-runners TL's recommendation · **Context:** the pre-arm audit finding (`docs/handoff/2026-07-02-FINDINGS-pre-arm-audit-CF-env0-gap-plus-cross-TL-items.md`) + ADR-0009.

## The question
The Cloudflare spawn Worker — the PRIMARY substrate (ADR-0009) — injects the per-job CAS PAT directly into the untrusted container env as `CLW_TOKEN` (`deploy/cloudflare/src/lib.ts:183`). C2c env-0 (deliver-via-single-use-cred-ticket) is implemented on the **fabricd/Northflank** path only, not the CF path. Port env-0 to the CF Worker now, or accept `CLW_TOKEN`-in-env for launch?

## The decision — Option B
**Accept `CLW_TOKEN`-in-env on the CF path for launch. Port env-0 to the CF Worker LATER, bundled into the already-booked namespace/prefix-scoping design pass (scheduled after the owner's arm-deploy).**

## Why it's acceptable for launch (the risk is bounded)
The PAT exposed in the untrusted env is:
- **Intra-tenant only** — scoped to the job's OWN tenant; cross-tenant isolation is enforced by CAS URL routing, NOT this PAT (so it cannot reach another tenant's cache).
- **Short-lived** — expires with the lease (ttl_seconds, #260/#590).
- **Revoked on completion** — killed server-side on every terminal path.

So the residual exposure is the same "a job can read/write (poison) its OWN tenant's cache" posture already accepted (pinned by `mint_pat_is_tenant_scoped_read_write_intra_tenant_poison_accepted`), plus an exfil-and-reuse window of ~job-duration until revoke. No new cross-tenant hole.

## What "later" means (not "never")
The CF env-0 port is a TRACKED line-item in the namespace-scoping design pass (owner: corelink-runners TL, with Server TL for CAS addressing + Workspaces TL for the clw entrypoint). It closes the "secrets brokered, never on the box" principle gap on the primary substrate. It is scheduled, not dropped.

## Not affected
The fabricd/Northflank path already has env-0 (fabricd is fail-closed + cred-ticket). This decision is specific to the CF-native Worker path.

— corelink-runners TL (recording the owner's ratification)
