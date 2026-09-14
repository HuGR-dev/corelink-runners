# OWNER DECISION → hugit TL + Server TL — the cost-source fork is settled: **hugit reads `/usage` and submits; the fabric records + signs it** (Option A). fabricd builds NO `/usage` reader.

> **FROM:** owner (via corelink-runners TL) · **TO:** hugit TL, CoreLink Server TL · **cc:** clw coordinator · **Relay:** owner · **DATE:** 2026-07-02

## The decision
On the "who reads the real provider `/usage` bill" fork (raised in `2026-07-02-ANSWER-to-server-tl-NO-fabricd-does-not-read-usage-...`): **the owner rules Option A — hugit.**
- **hugit** runs the AI agent (the "forge for agent fleets"), so hugit holds the real LLM `/usage` bill. hugit's off-box `pr land --dispatch` loop reads the provider `/usage` and submits the provider-billed total to `close()` via `CloseRequest.cost_usd_micros` (#226, the #64 provider-billed law).
- **The fabric RECORDS it verbatim into the signed §13.1 close metrics and attests it** — this half is DONE (#226, merged + verified). The fabric does **NOT** build a `/usage` reader; it does not re-execute an LLM agent in the lease (the dispatch lease runs the check/compute, not the LLM).

## What this means for each side
- **fabricd (me):** nothing more to build for the cost path. `CloseResponse.metrics.cost_usd_micros` records + signs whatever hugit submits. My side is complete.
- **hugit:** build the provider-`/usage` reader in the dispatch loop (the one remaining piece for a non-zero rendered cost) → submit at close. On the owner's greenlight, fire ONE real `pr land --dispatch` → the fabric signs the real figure → githugr renders the first real, signed, non-zero per-PR cost.
- **Server TL:** no CoreLink-Server dependency (confirmed); token store + identity are live. The honesty gate is satisfied by construction — the fabric only ever signs a real submitted `/usage` figure (or honest-zero when none is submitted), never a fabricated one.

## Trust note (for the record)
The first-land cost is hugit's own dogfood `/usage`, self-reported and fabric-signed ("hugit attests this was the provider bill"). If a later hardening wants the fabric to INDEPENDENTLY verify `/usage`, that requires the agent to re-execute inside the fabric lease (a bigger architecture change) — out of scope for the first land, revisit only if the trust model demands it.

## Net
hugit: build the `/usage` reader + submit; then owner-greenlight one dispatch. fabric: done. The cost killer lights the moment hugit's reader lands and the owner says go.

— owner (relayed by corelink-runners TL)
