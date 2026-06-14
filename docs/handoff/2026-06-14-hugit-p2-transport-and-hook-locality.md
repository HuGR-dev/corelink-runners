# → hugit techlead: §13 production wiring — 3 open items (subscriber identity · P2 transport · multi-instance hook-locality)

**De:** corelink-runners techlead · **Para:** hugit techlead (via owner) ·
**Data:** 2026-06-14 · **Status:** ASK — code-side is done; these three gate the
move from M1 (forensic-log) to **P2 live envelope transport**. ·
**Contexto:** todos os 3 seams §13 estão FECHADOS no wire (§13.2 #28, §13.4 #5,
§13.5 #37 + contrato espelhado v1.3.0). O que falta é a PONTE DE PRODUÇÃO, não o mecanismo.

---

## Why now

Our side is feature-complete for §13: the `CaptureHook` mechanism, the JobClose
ack state machine, and the §13.5 abnormal-close partial flush all ship and are
green. At M1 the envelope is a **forensic log line**; the **real push to hugit is
the P2 transport WP**, and it is now the critical path. To build the fabric side
of P2 I need three answers from you. None require a contract amendment except
where flagged.

---

## Item 1 — Subscriber identity (STILL OPEN since 2026-06-12)

The credential seam from `docs/handoff/2026-06-12-hugit-envelope-credential-seam.md`
(P1/P2) is **still pending a hugit decision**. Recap: the `HookRegistry` requires
the envelope subscriber to authenticate with the **same credential registered at
acquire** — today that's the raw Bearer PAT of the acquiring tenant (Option A,
zero contract change). If your envelope-consumer is a different role/service with
a different credential than the acquiring orchestrator, every poll 503s.

> **Need:** confirm **Option A** (consumer presents the same tenant PAT) — or, if
> roles are split, pick **Option B** (CoreLink-issued per-tenant envelope
> credential, out-of-band, no frozen-type change). Option C (per-lease credential
> in `AcquireResponse`) is a frozen-type amendment — only if you require per-lease
> isolation. **Recommended: A** unless your producer assina com PAT diferente.

This is a one-line change at our acquire composition root; it blocks P2 wiring, not M0/M1.

## Item 2 — P2 transport contract (NEW — the core of this handoff)

At M1 the envelope is poll-drain: the endpoints
`GET /v1/leases/{id}/envelope/{events,meta}` are mounted, and the abnormal-close
flush writes a forensic log line. For P2 I need the **delivery contract** decided
so I build the right thing:

> **Q2a — delivery mode.** Does hugit **poll** the fabric (pull: you call the
> envelope endpoints on a schedule / on lease-close signal), or does the fabric
> **push** to a hugit sink (webhook/queue you expose)? Our M1 shape is pull; if you
> want push, I need the sink contract (URL/queue + auth).
> **Q2b — completion signal.** How does the consumer learn a lease closed so it
> knows to drain? Today there is no close-notification out of the fabric. Options:
> (i) you poll meta and observe the terminal state; (ii) we emit a lightweight
> close event to a sink. Your call drives whether I build an emitter.
> **Q2c — retention / drain window.** How long must the fabric retain a closed
> lease's envelope for the consumer to fetch it (the hook lives in memory today —
> see Item 3)? A bound here defines whether envelopes need a durable store.
> **Q2d — backpressure / ack.** Normal close is exactly-once with an ack from the
> live client. For P2 abnormal/forensic envelopes there is no live client — do you
> want an at-least-once delivery with consumer-side dedup by `lease_id`, or
> exactly-once with fabric-side durable state? (This interacts directly with Item 3.)

## Item 3 — NEW multi-instance limitation: §13.5 hook-locality (decision needed)

Surfaced by the post-go-live audit (now documented in
`docs/deploy/northflank-postgres-runbook.md` §5). **The `CaptureHook` registry is
in-memory, per fabric instance.** Under multi-instance (we are now LIVE at N≥2 on
the persistent PgLedger), the reaper instance that *wins* the terminal-transition
CAS is **not guaranteed** to be the instance holding the hook → on a mismatch, the
**abnormal-close partial forensic envelope is silently dropped**.

- **Billing impact: NONE** — hugit prices flat; the envelope is forensic
  provenance, never a billing input. A dropped partial envelope costs a forensic
  record, never money. A **normal** close is unaffected (served by, and finalizes
  on, the instance the client talks to).
- **Severity: M1-acceptable** — §13.5 is explicitly best-effort, and the loss is
  only the abnormal (expired/crashed) partial, only at N>1.

> **Need (decision):** does hugit's forensic/provenance story **require** the
> abnormal partial envelope to be reliably delivered at N>1? 
> - **If NO** (best-effort forensic is fine): we keep M1 behavior; this is just an
>   FYI and Item 2 can assume best-effort.
> - **If YES:** it becomes a P2 fabric WP — either **persist hooks** alongside the
>   lease (so any instance can flush on reap) or **route the flush to the owning
>   instance**. This is real work; I'll scope + prioritize it once you confirm the
>   forensic SLA. It also constrains Q2d (durable hook ⇒ exactly-once is feasible).

---

## What I do NOT need

- No change to `IntentMetrics` (§13.4) — the frozen conformance vector
  (`conformance/IntentMetrics.json`, sha256 `2d8d2215…`) stays untouched; any
  change is a two-repo amendment, owner+hugit gated, never unilateral.
- No change to the §13.5 wrapper markers (`close_reason`, `capture_incomplete`) —
  ruled Option B, implemented, contract mirrored v1.3.0. This handoff is transport,
  not shape.

## ACTION REQUESTED

> **hugit techlead, route via owner:** (1) confirm subscriber identity (Item 1: A/B/C);
> (2) answer the P2 transport contract (Q2a–Q2d); (3) rule on the hook-locality
> forensic SLA (Item 3: best-effort OK, or require the P2 durability fix). With these
> I build the fabric side of the live envelope transport. No frozen-type change is
> implied unless you choose Item-1 Option C.

— roteado via owner; nenhum `path`/`git`-dependency entre repos.
