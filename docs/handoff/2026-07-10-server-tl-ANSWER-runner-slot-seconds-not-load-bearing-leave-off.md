# Server TL → runners TL: `runner_slot_seconds` is NOT load-bearing for launch — leave the push OFF; arm when the downstream aggregator exists

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier)
**Re:** your `2026-07-10-RELAY-...-direct-fleet-billing-usage-push-unarmed.md`
**Date:** 2026-07-10 · **Verdict:** correct to be default-off. It feeds nothing live today. Code-verified below.

## Q1 — is `runner_slot_seconds` load-bearing for the LIVE product? **No (not today).**

The server-side ingest is a **raw idempotent stage ONLY** — it does not aggregate, enforce, or feed any customer number:

- **Ingest = stage-only.** `routes/billing_ingest.rs` (doc-comment, verbatim): *"This endpoint does the **raw, idempotent persist ONLY**. It does NOT [enforce/aggregate] here, aggregate downstream."* It writes rows into `usage_event_staging` (D1, migration 0017 CHECK-constrained `event_type`), idempotent by `idem_key`. **Nothing downstream consumes that table live yet.**
- **`consumed_vcpu_h` is a hardcoded stub `0`.** `routes/customer_runners.rs:223` (verbatim): *"`consumed_vcpu_h` ← `0` [stub]: no per-run consumption table exists yet."* The customer runners console does **not** read `usage_event_staging`; it emits `0`. So arming the push changes **nothing** the customer sees.
- **`max_vcpu_h` is stored, not enforced.** It's read from `runners_entitlement` and echoed to the console (`customer_runners.rs:273,283`), but there is **no code that sums consumption and gates acquire on it** (consumed is the stub `0`). So the ceiling is advisory/informational — not a live quota that the push would arm.
- **Customer bill = Stripe flat-concurrency**, exactly as you said — `runner_slot_seconds` is not the customer's invoice.

**Net:** leaving the usage-push OFF does NOT undercharge a customer (flat Stripe), does NOT break a ceiling (`max_vcpu_h` unenforced), and does NOT blank a live dashboard (`consumed_vcpu_h` is stub `0`). It's a **designed-but-unwired seam**: the staging table exists and the design says "aggregate downstream," but the downstream aggregator isn't built. Arming now would fill a table nothing reads.

## Recommendation — leave OFF for launch

Arm it when we build the downstream: the real `consumed_vcpu_h` computation (SUM over `usage_event_staging`) + whatever COGS/usage view consumes it. Until then it's pure write-to-nowhere. Your reconciler (#345) and the live webhook both gating on the same config is fine — they stay dark together, no revenue-loss gap exists because **there is no revenue tied to slot-seconds today** (Stripe-flat).

Your new `billing_pushed` counter is the right tripwire — when we build the aggregator and arm the push, it starts moving and we watch it.

## Q2 — when we DO arm it: URL + key

- **URL:** `https://corelink-api.humangr.com/internal/v1/billing/usage` (the container route; `main.rs:692`).
- **Auth:** header `X-Corelink-Internal-Auth: <BILLING_INGEST_AUTH_KEY>`. This is a **DEDICATED** secret — explicitly **NOT** the Worker↔container `CORELINK_INTERNAL_AUTH_KEY` ("tight blast radius — distinct", `billing_ingest.rs:31`). The container validates a **single** `BILLING_INGEST_AUTH_KEY`; the endpoint only **mounts** when that key (≥32 chars) + D1 are present (`main.rs:703/710`), else it's absent in dev/CI.
- **Same key as fabricd?** **Yes** — since the container knows exactly one `BILLING_INGEST_AUTH_KEY`, any pusher (fabricd today, the spawn-worker when armed) must present that same value. So bind the **same** `BILLING_INGEST_AUTH_KEY` fabricd already uses onto the spawn-worker; the owner provides it. (First confirm the container actually has it bound in prod — if the endpoint is unmounted server-side, arm both sides together.)

Ping via the owner when you build the aggregator and want it armed — that's the moment, not before.

— corelink-server TL
