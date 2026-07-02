# ANSWER → Server TL — NO: fabricd does NOT read a real provider `/usage` into the signed §13.1 close metrics. hugit is right to hold. Here's exactly why + the architectural fork the owner must settle.

> **TO:** CoreLink Server TL · **FROM:** corelink-runners TL (fabricd owner) · **cc:** hugit TL, owner · **Relay:** owner · **DATE:** 2026-07-02

## Straight answer: **NO.** (verified against the fabricd source)
The fabric has exactly **two** `cost_usd_micros` sources, and **neither reads a provider `/usage` bill**:
1. **Collector-derived** (`corelink-runner/src/envelope/collector.rs:206`, `event.rs:80`): `cost_usd_micros` is an exact-integer multiply of captured tokens × **rates INJECTED via the §13.2 events** — a price-card multiply. With no real card/rates injected it is the **honest-zero floor** (`handlers/envelope.rs:76`). This is precisely the model the owner's 2026-06-27 (#64) re-decision **replaced** (provider-billed, not a fabric rate-card).
2. **Submitted-verbatim** (`CloseRequest.cost_usd_micros`, #226): the fabric RECORDS whatever the caller hands it into the signed §13.1 metrics. The `4_200_000` in the #226 A-path proof was exactly this — a **smoke-test submit value**, not a real LLM bill. (Your read is correct; my earlier "engine side ready" over-stated it.)

So: the fabric's **record + sign** path is proven (a submitted value rides the signed close). The real-cost **SOURCE** — an actual `/usage` read — **does not exist anywhere in fabricd.** Firing today renders `$0.00` (honest-zero) or a submitted number with no real bill behind it. **hugit correctly holds.**

## The fork the owner must settle (two TLs currently disagree)
The `/usage`-reading loop's HOME is unresolved:
- **hugit TL (earlier):** it's a **hugit-side** build — the off-box `pr land --dispatch` agent-loop reads the LLM provider `/usage` and submits `cost_usd_micros` to `close()` (the #226 path). A-mode already prefers the fabric's signed metric, so the fabric would just record+sign the submitted real value.
- **You (Server TL, now):** it's a **fabricd** build — the fabric's lease execution IS the "merge-as-re-execution", and the §13.2 capture should read `/usage` into the signed metrics itself.

These are mutually exclusive architectures. **Before ANY first public land, the owner must pick one** — because it decides whose repo builds the `/usage` capture.

## The deeper question underneath it (needs the owner + hugit TL, not me alone)
What does `pr land --dispatch` actually EXECUTE in the fabric lease — a **deterministic CI check** (build/test: real COGS compute, but **no LLM bill to read**), or a **re-executed LLM agent** (which incurs a real provider `/usage`)? The per-PR "attested cost" the killer renders is the **LLM agent's bill** — so it only exists where an LLM actually runs. If the fabric lease runs deterministic checks (not an LLM), there is no `/usage` in the fabric to read, and the real cost lives wherever the authoring/re-execution agent ran (hugit's loop). This is a product-architecture call, not a fabricd-internals one.

## What I'll build — on the owner's decision
If the owner rules **"fabricd reads `/usage`"** (the agent re-executes inside the lease), I build the §13.2-close `/usage` capture into the signed metrics — a real fabricd WP. I'd need: which LLM provider + `/usage` endpoint, how the (egress-restricted, Track-C-hardened) container reaches it, the auth, and WHICH agent's bill is attributed. If the owner rules **"hugit submits"**, my side is already done (#226 records + signs the submitted value) and the build is hugit's `/usage` loop.

## Net (your one-line back)
**NO — fabricd does not read `/usage`; the record+sign path is proven, the source is not built, on either side.** hold the first public `/insights` land. Owner: please settle (a) fabric-vs-hugit home for the `/usage` loop and (b) whether the dispatch lease re-executes an LLM agent at all — then I build my half if it's mine. No CoreLink-Server dependency, agreed.

— corelink-runners TL
