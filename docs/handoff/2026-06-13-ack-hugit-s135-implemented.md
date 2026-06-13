# → hugit: ACK — §13.5 abnormal-close flush IMPLEMENTED (Option B). All three §13 seams closed.

**De:** corelink-runners techlead · **Para:** hugit techlead (via owner) ·
**Data:** 2026-06-13 · **Status:** §13.5 implemented + merged to main; no shape divergence. ·
**Em resposta a:** `hugit/docs/handoff/2026-06-13-envelope-abnormal-close-ruling.md` (Option B ruling).

---

## ACK — §13.5 implemented exactly to the ruling

Option B (best-effort partial flush, NOT drop) is implemented and on `corelink-runners` main.
No contract divergence — no ping needed. Per your §5, our `reaper.rs`/`JobClose` shape matched the
ruling's assumptions; we only added the additive `close_reason` field.

| Ruling requirement | Implementation |
|---|---|
| Flush on **Expired** + **Crashed** (do NOT drop) | `reaper::flush_partial_envelope` is called on both the deadline-reaper (`reap_once`) and crash-sweep (`surface_crashes`) paths, after teardown→transition→`record_slot` |
| Reuse the existing envelope + 2 close-metadata fields | `CloseReason { Normal, Expired, Crashed }` (wrapper-level, serde `snake_case` → `normal\|expired\|crashed`) on `CloseOutcome`; `capture_incomplete:true` set unconditionally by `close_abnormal` |
| **§13.4 `IntentMetrics` vector UNTOUCHED** | confirmed — `close_reason` is on the close/`JobClose` machinery, never inside `IntentMetrics`; the §13.4 vector + type are byte-unchanged (the drift tripwire still green) |
| Fire-and-forget, **no ack**, teardown doesn't wait | the flush runs AFTER teardown; `close_abnormal` arms no ack window; on its exactly-once `Err` (a normal close already consumed the hook) the sweep logs + continues — **no second envelope, no double-free** |
| Dedup by `lease_id` | `HookRegistry::close_handle_any` extracts the hook exactly once; the ledger transition is atomic + mutually exclusive (`Held→Released` vs `Held→Expired\|Crashed`), so a normal close and an abnormal flush can never both fire |
| **Redaction identical — no exemption** | the partial goes through the SAME `collector.finalize` redaction write-path as a normal close; the forensic record carries only the finalized `IntentMetrics` SUMMARY (tokens/tool_calls/cost/wall/active), never raw trajectory |

## M1 delivery semantics (honest)

There is **no live push transport at M1** (the envelope is poll-drain; a reaped lease has no live
poller). So at M1 the flush **finalizes** the partial envelope (markers + redaction) and emits it
to the available forensic sink — a **single structured log line** (`lease_id`, `tenant`,
`close_reason`, `capture_incomplete`, the metrics summary). **hugit consumes at P2** when the live
envelope-transport seam (interop §2) is wired — no hugit code change now, exactly as your §4 scoped
it. The finalize/markers/dedup/redaction are all final; only the transport hop is P2.

## The three §13 seams — CLOSED on both sides

| Seam | Status |
|---|---|
| **§13.2** envelope credential (Option A — same tenant PAT) | ✅ ratified + wired (#28) |
| **§13.4** `IntentMetrics` conformance vector | ✅ twin merged byte-identical (#5); drift tripwire live both repos |
| **§13.5** abnormal-close partial flush (Option B) | ✅ implemented + merged (this) |

Nothing §13 is open from our side. We'll mirror §13.5 into our contract copy
(`docs/spec/hugit-integration-contract.md`) in sync with your contract amendment; ping us if your
mirrored §13.5 wording differs from the above so the two copies stay byte-aligned.

— roteado via owner; nenhum `path`/`git`-dependency entre repos.
