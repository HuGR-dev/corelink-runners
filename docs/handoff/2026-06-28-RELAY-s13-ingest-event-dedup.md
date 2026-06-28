# RELAY → owner / hugit-techlead — §13 ingest has no event-level dedup (needs a wire idempotency key; coordinate)

> **From:** autonomous audit loop (round 4) · **To:** owner → hugit-techlead · **Date:** 2026-06-28
> **Why a relay, not a patch:** the clean fix adds an idempotency key to `IngestEvent` — a §13 wire type that **hugit's dispatch submits**. A wire-shape change is coordinated both-sides, never added unilaterally (wire-contract law). The server-side-only alternatives are heuristic; the correct fix is a caller-supplied key.

## Finding (low — audit/dashboard only, billing-immune)
The §13 ingest path (`handlers/envelope.rs`) has no event-level dedup. `IngestEvent` carries no event-id / sequence / nonce, and `collector.observe()` accumulates via `saturating_add` with no prior-seen guard. The HMAC ingest token is deterministic per `(secret, lease_id)`, so a **replayed POST** (network retry of hugit's dispatch) passes auth, finds the live hook, and **double-counts** tokens/tool-calls; the turn-boundary checkpoint then persists the doubled totals.

**Why low (verifier-confirmed):** billing is flat-tier on `runner_slot_seconds` (wall-clock) and the aggregator dedups on `idem_key = BLAKE3(lease_id‖period)`, so the **Stripe-facing charge is immune**. `cost_usd_micros` is documented "NEVER a billable meter; for trust/audit." Harm is confined to dashboard display / anti-abuse heuristics / audit reconciliation. §13 contains no explicit ingest-side dedup requirement today.

## Recommended (coordinated) change
1. Add `event_id: Option<String>` (or a monotone `seq: u64`) to `IngestEvent` — **coordinated** on both the fabric side (`corelink-fabric-api` / the ingest handler) and **hugit's dispatch submitter** (which must populate it deterministically per event).
2. Fabric side: a per-hook **bounded** `HashSet<String>` (or `last_seen_seq`) — skip-and-200 a duplicate; cap the set at the buffer capacity (no unbounded memory — pairs with the round-4 collector cardinality cap already landed).
3. Conformance: extend the §13 vectors + the roundtrip test for the new field.

## Interim mitigation already landed (this loop)
The round-4 collector **cardinality + name-length cap** (PR `…ingest-dos…`) bounds the per-lease memory + checkpoint size regardless of replay, and the **ingest body cap** (1 MiB) bounds a single submission. So the *DoS* surface is closed; this relay is specifically the **double-count accuracy** issue, which needs the wire key.

No code changed in this repo for the dedup itself. Tracked in `2026-06-28-audit-loop-round-4.md` (#7).
