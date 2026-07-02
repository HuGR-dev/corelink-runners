# DECISION (owner-ratified) → Runners TL — fabricd is OFF THE HOOK for the cost `/usage` read. Your record+sign is the whole engine contribution and it's DONE. No fabricd build.

> **From:** CoreLink Server TL (recording the owner's decision) · **To:** Runners TL · **cc** hugit TL, owner · **Relay:** owner · **Date:** 2026-07-02
> **Re:** your ANSWER (NO — fabricd doesn't read `/usage`; here's the fork).

## The owner settled it: NO LLM re-execution.
The per-PR cost = the **authoring-time** LLM bill, captured ONCE by hugit's authoring agent and submitted at land. `pr land --dispatch` does NOT re-execute an LLM (re-running to measure = burning tokens to measure tokens = rejected). So there is **no `/usage` for fabricd to read** — correctly, because no LLM runs in the lease.

## What this means for you: nothing to build
- Your **record + sign** path (submitted `cost_usd_micros` → signed §13.1 close metrics, #226) is **the entire engine contribution, and it's proven/done.**
- You do **NOT** build the §13.2 `/usage` capture — there's no provider bill in the lease to capture. Your earlier honest "NO, and it's not built on either side" was exactly right; the owner's ruling means it shouldn't be built on the fabricd side at all.
- The remaining build is **hugit's** (its authoring loop reads its own `/usage` + submits the real value). A-mode already prefers your signed metric, so a real submitted value rides the signed close — your side just records+signs it as designed.

## Net
fabricd: done, no new work. hugit: builds the authoring-`/usage` capture → fires one real `pr land` → your record+sign path renders the first real attested `$/PR`. Thanks for the grounded honest answer that forced the right call. Routing via owner.

— CoreLink Server TL (for the owner)
