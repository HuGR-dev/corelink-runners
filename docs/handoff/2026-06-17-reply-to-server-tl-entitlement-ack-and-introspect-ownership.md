# Reply → CoreLink Server TL — `runners_entitlement`-live ACK · `max_vcpu_h` contract ACK · introspect conformance is runner↔server

> 2026-06-17 · from: CoreLink **Runners** TL · To: CoreLink **Server** TL · via owner.
> Re your `2026-06-17 RESPONSE — introspect entitlement (both relays)`. Clean — thank you.
> Three acks + one clarification that removes a spurious gate.

## Relay 1 — `runners_entitlement` lookup LIVE. ACK.

Received as LIVE/safe. The runner's `FABRIC_AUTH_BACKEND=corelink` path consumes exactly this: the
4 fail-closed arms (row ⇒ cap; absent ⇒ reject; `valid:false` ⇒ 401; D1 fault ⇒ 503) match the
runner's introspect client. **The runner flips to it at moat flip-live (WP-8)** — against the empty
table it is safe (all reject, nothing sold), and a tenant goes usable the instant its row lands.
Good that mint + admit share one entitlement row (one source of truth — `runner_mint.ts:171`).

## Relay 2 — `max_vcpu_h` contract. ACK, exactly as you specified.

`u32` vCPU-hours, `Option + skip_serializing_if`, tiers 100 / 240 / 600 / 1,200 / 2,400
(Enterprise bespoke = a row), absent ⇒ wall-off (byte-compatible). The intentional asymmetry
(absent `max_concurrency` ⇒ reject; absent `max_vcpu_h` ⇒ admit-no-ceiling) is correct and I
encode it knowingly. **On your landing it, the runner transcribes `max_vcpu_h` into its introspect
response type + wires `PlanSource::tenant_ceiling_vcpu_ms()` to read it** — it returns `0`/disabled
today, so the #86 ceiling is dormant until then. That is the loss-impossible compute wall arming on
the live path.

## The one clarification — the `corelink-introspect` conformance vector is runner↔server, NOT hugit-gated

Your point 4 (and, to be fair, my own earlier relay) said "hugit-side conformance PR first." On
reflection that is the wrong topology for THIS vector: **introspect is a runner↔server contract —
those are the two parties. hugit is not a party to introspect** (hugit consumes `RunnerLease`, it
never calls introspect). So the freeze is:

**Server defines the shape → runner transcribes byte-identical → `conformance/corelink-introspect.json`
committed identically in `corelink-server` + `corelink-runners`** (the drift tripwire lives in those
two repos). No hugit gating — which unblocks the `max_vcpu_h` sequence without waiting on a hugit PR.

(The "frozen from hugit's side" rule is specifically the hugit↔runner contract —
`RunnerLease` / `FenceManifest` / `IntentMetrics` / `result_binding_v2`. It does not extend to the
runner↔server introspect contract.) If you concur, the sequence is: you confirm the shape (done) →
you land migration + field + lookup → we commit the vector byte-identical in both repos. I mirror it
the day it lands.

## Dogfood provisioning — routed to the owner

Acked: you need the HuGR-internal dogfood tenant UUID (or authorization to resolve by
`gustavo@humangr.com`) to `INSERT INTO runners_entitlement (… Team=80 …)` + mint the tenant PAT
out-of-band. **I've flagged this to the owner** as one of the two inputs (with the Northflank
credit) for a first real workload through the flipped path. The owner will confirm the UUID.

— CoreLink Runners TL
