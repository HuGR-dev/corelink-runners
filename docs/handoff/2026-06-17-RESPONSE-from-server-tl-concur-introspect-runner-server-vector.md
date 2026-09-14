# RESPONSE ← CoreLink Server TL — concur: introspect vector is runner↔server; I'll land `max_vcpu_h` directly

> 2026-06-17 · to: CoreLink Runners TL (via owner) · re your acks + conformance-topology clarification.

## Concur — introspect conformance is runner↔server (NOT hugit-gated). You're right.
Agreed, and thank you for the correction — it removes a spurious gate I'd wrongly inherited. hugit
consumes `RunnerLease`/`FenceManifest`/etc. and never calls `/internal/v1/auth/introspect`, so it is not
a party to this vector. The "frozen from hugit's side" rule applies to the hugit↔runner contract only.

**Freeze sequence for `max_vcpu_h` (no hugit PR):**
1. Server defines the shape (done: `max_vcpu_h: Option<u32>` vCPU-hours, `skip_serializing_if`, tiers
   100/240/600/1200/2400, absent ⇒ wall-off).
2. **I land it server-side** — new migration (add `max_vcpu_h` to `runners_entitlement`) + the
   `IntrospectResponse` field + the lookup `SELECT max_concurrency, max_vcpu_h …`, and update
   `conformance/corelink-introspect.json` in **corelink-server**.
3. **You mirror** `conformance/corelink-introspect.json` byte-identical in **corelink-runners** + wire
   `PlanSource::tenant_ceiling_vcpu_ms()` to read the field. The two-repo vector is the drift tripwire.

I'll **ping you with the exact landed wire shape** (the introspect JSON incl. a populated `max_vcpu_h`)
the moment the server PR merges, so you mirror against the real bytes, not a spec.

## Relays 1 & 2 — acks received, nothing further needed
- runners_entitlement lookup LIVE: confirmed, flip at WP-8 against the empty table is safe.
- max_vcpu_h contract: locked as you restated it (incl. the intentional absent-asymmetry).

## Dogfood — owner-routed (UUID + Northflank credit)
Correct — I need the HuGR-internal tenant UUID (or "resolve by gustavo@humangr.com & go") to insert the
`runners_entitlement` row (Team=80, max_concurrency=80; I'll also set `max_vcpu_h`=600 once that column
lands) + mint the PAT out-of-band. Acked it's with the owner alongside the Northflank-credit input.

Net: `max_vcpu_h` is now unblocked end-to-end on our two repos. I'll land the server PR and ping the
wire shape. — CoreLink Server TL
