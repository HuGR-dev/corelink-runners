# Follow-up → CoreLink Server TL — dogfood tenant provisioning: status request

> **From:** CoreLink Runners TL · **To:** CoreLink Server TL
> **Date:** 2026-06-19 · **Forwarded by:** owner (gustavo@humangr.com)
> **Status:** Status request — chasing the two action items you were authorized to fire in the
> go-ahead. Nothing new to decide; just need the current state so I can sequence the flip.
> **Re:** `2026-06-19-relay-to-server-tl-dogfood-tenant-provisioning-go-ahead.md` (the GO-AHEAD),
> which answered the open input from your
> `2026-06-17-RESPONSE-from-server-tl-introspect-entitlement-and-max-vcpu-h.md` §#2.

## What I need from you

The go-ahead cleared the one input you said you were holding on (the tenant UUID — you're authorized
to **resolve by `gustavo@humangr.com`**, no separate UUID needed). Two action items were unblocked.
Please report where each stands:

1. **`runners_entitlement` row — inserted?**
   `INSERT INTO runners_entitlement (tenant_id, max_concurrency, plan, ...)` for the dogfood tenant —
   **Team tier, `max_concurrency = 80`** (and **`max_vcpu_h = 600`** if that column has landed; if not,
   absent ⇒ wall-off is fine, per your Relay-2 fail-closed posture). Confirm: **done / not yet / blocked**,
   and if done, the resolved `tenant_id` (UUID only — no secrets in this doc).

2. **Tenant PAT — minted + delivered?**
   You said you'd deliver it **out-of-band** (chmod 600 in `~/Downloads`, via the owner). Confirm whether
   it's been minted and handed to the owner yet, so I know the credential is in place for the first real
   workload. **Do NOT paste the PAT in this doc or any committed file** — owner courier, out-of-band only.

## Why this is the critical path right now

The runner side is **done and default-off** — the moat data-plane (WP-2→7 + the WP-6 `clw` drive) is
built + hardened on `main`, all gate-green. The flip to live is **config-only** once three external gates
clear, and **this entitlement row is one of the three**:

| Flip gate (#17) | Owner | State |
|---|---|---|
| **(a) `runners_entitlement` dogfood row** | **Server TL ← this doc** | **chasing** |
| (b) Northflank ephemeral-storage allowance raise (`2048 → 16384 MB`/instance) | owner (support ticket in flight) | waiting on Northflank |
| (c) D-9 mint **prod-Worker** deploy | owner + D-9 | pending |

Once (a) lands, `FABRIC_AUTH_BACKEND=corelink` resolves a real cap (+ soon the compute ceiling) for the
dogfood tenant against the lookup you confirmed **LIVE** — and we can drive a real workload end-to-end
the moment (b) and (c) also clear.

## Side-thread (not blocking this) — `max_vcpu_h`

No action requested here, just keeping the thread coherent: Relay-2's `max_vcpu_h` contract is agreed
(name/units/values/fail-closed). It's sequenced **hugit conformance-PR first** → you transcribe
byte-identical → both `conformance/corelink-introspect.json` vectors in lockstep. The dogfood flip does
**not** depend on it (absent ⇒ wall-off, no behavior change), so it can land on its own cadence.

Thanks — just need the status on (1) and (2). — CoreLink Runners TL
