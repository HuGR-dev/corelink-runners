# Cross-repo closure: all four 2026-06-14 hugit §7/§13 seams CLOSED both-sides

**Recorded by:** corelink-runners techlead · **Date:** 2026-06-14 ·
**Terminal message:** hugit
`docs/handoff/2026-06-14-reply-corelink-v2-vector-acked-and-final-asks.md`
(no ask back — loop closed from hugit's side).

This note closes the provenance chain for the four 2026-06-14 fabric-integration
handoffs. Nothing is open; the fabric builds **nothing new** beyond what is shipped.

| Seam | Final state | Evidence |
|---|---|---|
| **§7.1 `result_binding_sig_v2`** | **Ratified + mirrored both sides.** Vector `conformance/result_binding_v2.json` (sha `600c99b5…`) is byte-identical in hugit, in their manifest + X4 drift tripwire (`acceptance_x4_wire VECTORS[4]`), landing **hugit PR #120**. The sha-pin is the active cross-repo lock; hugit's full ed25519 verify+tamper wires when its **P2 attestation-verify** path lands (no ed25519 dep there today, no live X8 fold yet). | ours: #52 (`d365af3`); contract v1.4.0 |
| **§7 M1 attestation scope** | Accepted. Axes are runner-asserted (our `AttestationChain.model=""`); hugit's X8 writer tenses them "runner-asserted, not fabric-observed" until FC3. | hugit reply §2 |
| **§13.2 trajectory ingest** | Accepted as-built (Option A / HTTP POST, the `TranscriptEvent` variants). hugit adopts the endpoint at its live-runner last mile. | ours: §13.2 shipped |
| **§13 P2 transport + hook-locality** | All four decisions = our existing M1 shape (PULL · poll `meta` · at-least-once + hugit dedups · best-effort hook-locality). We confirmed we do **not** build the declined durability fix. | hugit reply §4 |

## The two optional WPs — DECLINED by hugit (do not build)

- **(a) per-turn `model` id on `TurnMeta`** — hugit ruled **skip**: model is ~constant
  per agent-loop/lease, so per-model cost attribution resolves at orchestration level
  without a per-turn field. Would be a §13 amendment + `TurnMeta` vector bump; not
  worth the churn now. hugit raises a scoped WP if ADR-0001 compaction ever needs it.
- **(b) post-close envelope drain window (≤15 min)** — hugit ruled **not needed**:
  final metrics return in the acked `CloseResponse` (exactly-once); progressive events
  drain during exec; abnormal/forensic stays best-effort (zero billing impact). hugit
  asks for the bounded retain only if a future P2 consumer needs an after-terminal poll.

**Net:** no frozen-type change; `IntentMetrics` (`2d8d2215…`) + the chain pre-image
untouched. No further runner-side action on these seams. Any re-open is hugit-initiated.
