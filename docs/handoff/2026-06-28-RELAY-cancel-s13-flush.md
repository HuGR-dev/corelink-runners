# RELAY → owner / hugit-techlead — §13 partial-envelope flush on CANCEL (needs CloseReason::Cancelled + a product decision)

> **From:** autonomous audit loop (round 6, cross-path lens) · **To:** owner → hugit-techlead · **Date:** 2026-06-28
> **Why a relay, not a patch:** the clean fix needs a new `CloseReason::Cancelled` variant — and that enum's serde strings are documented "**stable across the hugit seam**" (`envelope/close.rs:52-59`), i.e. the frozen §13 wire. Adding a wire variant is coordinated both-sides, never unilateral; and reusing `Crashed`/`Expired` for a cancel would be a **dishonest forensic label**. There is also a genuine product question below.

## Finding (high) — cancel does not flush the §13 capture hook
The cancel handler (`leases.rs`, the Held→Released real-transition arm) does `record_slot → teardown_lease → revoke_pat_for → forget_lease` but **never flushes the capture hook** — `forget_lease` unregisters it with no close signal. The round-5 reaper fix made expire (`reap_once`) and crash (`surface_crashes`) flush the partial envelope BEFORE `forget_lease`; **cancel is the third abnormal exit and has no flush at all**, so a cancelled lease's in-process partial §13 metrics are silently discarded (no `capture_incomplete` marker, no forensic record).

## Two coupled decisions for the owner / hugit-techlead
1. **Product:** should a *voluntary* cancel flush a partial §13 envelope? A cancel is the owner aborting their OWN lease — unlike expire/crash (involuntary). Reasonable either way: (a) flush it for forensic completeness (symmetric with expire/crash), or (b) treat a voluntary cancel as "discard" (no forensic). The verifier rated this high on the assumption (a) is wanted.
2. **Wire (if (a)):** add `CloseReason::Cancelled` (serde `"cancelled"`) + `AbnormalKind::Cancel` in `envelope/close.rs`, coordinated with the **hugit side's** transcription of `CloseReason` (so hugit's verifier accepts the new string), plus a conformance note. Then the cancel arm calls the (made `pub(crate)`) `flush_partial_envelope` BEFORE `forget_lease`, mirroring the reaper.

## What this loop did NOT do (and why)
Did not add `CloseReason::Cancelled` (frozen-seam change) and did not mislabel a cancel as `Crashed`/`Expired` (dishonest). Both await the decision above. Tracked in `2026-06-28-audit-loop-round-6.md` (#1). The reaper expire/crash flush (the related involuntary-exit case) IS fixed (#213).
