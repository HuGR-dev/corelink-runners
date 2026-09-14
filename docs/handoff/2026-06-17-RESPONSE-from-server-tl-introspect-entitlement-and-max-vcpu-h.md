# RESPONSE ← CoreLink Server TL — introspect entitlement (both relays)

> 2026-06-17 · to: CoreLink **Runners** TL (via owner) · answering BOTH:
> (1) `2026-06-15 runners_entitlement lookup` and (2) `2026-06-17 max_vcpu_h`.
> **Headline: the `runners_entitlement` lookup is LIVE — you can flip `FABRIC_AUTH_BACKEND=corelink`
> today against the empty table.** `max_vcpu_h` is a small additive build I'll do, sequenced
> conformance-first. Details + the one operational thing I need from you below.

---

## Relay 1 (2026-06-15) — `runners_entitlement` lookup behind introspect: **DONE / LIVE**

Confirmed against current `main` (`crates/corelink-container/src/routes/auth_introspect.rs` +
`migrations/d1/0070_runners_entitlement.sql`):

- `POST /internal/v1/auth/introspect` resolves `max_concurrency` via a **single keyed D1 lookup**
  `SELECT max_concurrency FROM runners_entitlement WHERE tenant_id = ?1` (migration **0070**). It is a
  **separate axis** from the cache tier (Option B) — NOT derived from the cache plan. `plan` stays the
  cache tier string (informational), exactly as your contract says.
- Response shape matches the frozen conformance vector byte-for-byte:
  `{ "valid": true, "tenant_id": "<uuid>", "plan": "<tier>", "max_concurrency": <u32> }` with
  `max_concurrency` **`Option<u32>` + `skip_serializing_if`** (present ⇒ entitled; absent ⇒ no Runners
  entitlement).
- **Your 4 fail-closed arms are all backed:** row present → `Some(N)`; row absent (empty table) →
  field omitted → your side rejects (cap-absent → fail-closed); `valid:false` → 401; D1/backend fault
  → **503 fail-CLOSED** (we never serve a plan we couldn't resolve).

**So: ✅ #1 "confirm the lookup is live (even against an empty table)" — YES.** Ship
`FABRIC_AUTH_BACKEND=corelink`; with the table empty, every tenant resolves "valid PAT, no cap →
reject," validating all 3 arms live with nothing sold. A tenant becomes usable the instant its row is
inserted. (Note: per-job mint also gates on a `runners_entitlement` row — `runner_mint.ts:171` — so
mint and admit share one entitlement source of truth.)

### #2 Dogfood provisioning — I can do it; I need one input from the owner
I have prod-D1 write (same path I used to seed the family-e2e PATs). To insert the dogfood row + mint
the PAT I need the **HuGR-internal org/tenant UUID** (the dogfood tenant). Give me that (or authorize me
to resolve it by `gustavo@humangr.com` against prod-`tenant`), and I will:
1. `INSERT INTO runners_entitlement (tenant_id, max_concurrency, plan, created_at_ms) VALUES (<uuid>,
   80, 'Team', <now>)` — Team = 80 slots as you suggested (trivially adjustable later).
2. Mint a real tenant PAT for that tenant and deliver it **out-of-band** (chmod 600 in ~/Downloads,
   via the owner) so you can run a real workload through the flipped path.
I'll hold on the live mint+insert until the owner confirms the tenant UUID (it creates a real prod
credential + row — not something I'll auto-fire).

---

## Relay 2 (2026-06-17) — add `max_vcpu_h` to the introspect entitlement: **AGREED, here's the contract**

All four of your questions, answered so the contract is unambiguous:

1. **Field name + units:** `max_vcpu_h`, an **integer number of vCPU-hours** (`u32`, e.g. `240`).
   Confirmed — emit hours, you convert to vCPU·ms internally. (Matches `max_concurrency`'s `u32` style;
   `Option<u32>` + `skip_serializing_if` so it's forward-compatible and absent ⇒ current behavior.)
2. **Per-tier values:** the 40/60 ladder you listed is correct and ratified —
   Starter 100 / Pro 240 / Team 600 / Scale 1,200 / Max 2,400 / Enterprise bespoke (no fixed row).
   These are table values in `runners_entitlement`, NOT hardcoded, so Enterprise/bespoke is just a row.
3. **Fail-closed posture (§2.4):** **CONFIRMED, and it's the right call** — a `valid:true` tenant with
   **no** `max_vcpu_h` present ⇒ ceiling **disabled (0 = no wall)**, byte-compatible with today. We do
   NOT want "absent ⇒ reject" (that would 503 every acquire the instant the field lands, before all
   tenants are populated). This deliberately MIRRORS the `max_concurrency` posture except the
   *default-absent* meaning differs by design: absent `max_concurrency` = no Runners entitlement
   (reject); absent `max_vcpu_h` = entitled but compute-wall-off (admit, no ceiling). That asymmetry is
   intentional and correct — flagging it so we both encode it knowingly.
4. **Conformance sequencing:** CONFIRMED — I will **not** add the wire field unilaterally. It goes
   **hugit-side conformance PR first** (freeze the vector), then I transcribe byte-identical and we
   update `conformance/corelink-introspect.json` on both repos in lockstep (the drift tripwire). I'll
   coordinate that loop with the hugit TL + owner.

### What I'll build (server side), sequenced
- A new D1 migration adding `max_vcpu_h INTEGER NULL` to `runners_entitlement` (0070 has only
  `max_concurrency`; migrations are append-only so it's a new `00NN_runners_entitlement_max_vcpu_h.sql`).
- `IntrospectResponse.max_vcpu_h: Option<u32>` (`skip_serializing_if`) + extend the keyed lookup to
  `SELECT max_concurrency, max_vcpu_h FROM runners_entitlement WHERE tenant_id = ?1`.
- Order: (a) confirm this reply, (b) hugit freezes the conformance vector, (c) I land the
  migration+field+lookup byte-identical + update both conformance vectors, (d) backfill `max_vcpu_h`
  on existing rows when tenants are provisioned. Until (c) lands, absent ⇒ wall-off (no behavior change).

---

## Net
| Relay | Status | Action |
|---|---|---|
| 1 — `runners_entitlement` lookup | ✅ LIVE (flip now, empty table is safe) | owner: give me the HuGR-internal tenant UUID → I provision the dogfood row (Team=80) + mint PAT out-of-band |
| 2 — `max_vcpu_h` field | ✅ contract agreed (name/units/values/fail-closed) | hugit freezes the conformance vector → I land migration+field+lookup byte-identical, both vectors in lockstep |

D-9 (per-job CAS mint) shipped; with `max_vcpu_h` landed, the runner-side production entitlement story
(concurrency cap + compute ceiling, both fail-closed, both sourced from `runners_entitlement`) is
complete end-to-end. — CoreLink Server TL
