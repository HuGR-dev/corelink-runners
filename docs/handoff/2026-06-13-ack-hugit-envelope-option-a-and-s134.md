# → hugit: ACK — Option A (§13.2) ratified & wired; §13.4 twin merged (seam CLOSED both sides)

**De:** corelink-runners techlead · **Para:** hugit techlead (via owner) ·
**Data:** 2026-06-13 · **Status:** ACK — every requested action is DONE (cold-verified). ·
**Em resposta a:** `hugit/docs/handoff/2026-06-12-to-corelink-runners-envelope-reply.md`
(Option A decision) + `hugit/docs/handoff/2026-06-13-intentmetrics-twin-landed.md` (twin landed).

---

## ACK — Option A (§13.2 envelope credential seam)

Received and acted. The §13 envelope subscriber credential = the **acquiring tenant's
Bearer PAT** (ADR-0002, one HuGR machine PAT, no parallel auth domain). Every concrete
action you requested is already in `main`:

| Your request | Status | Evidence (on `corelink-runners` main) |
|---|---|---|
| (1) Promote the `leases.rs` assumption → **ratified** + pointer to your reply | ✅ done | `crates/corelink-fabric-server/src/handlers/leases.rs:246-253` — `CROSS-REPO SEAM — RATIFIED (hugit techlead, owner-ratified Gustavo, 2026-06-12; Option A "same tenant PAT")`, pointing to `…2026-06-12-to-corelink-runners-envelope-reply.md` |
| (2) Proceed with §13.2 M1 wiring | ✅ done | `CaptureHook` registered at acquire on the **Held** path (gated on a real transition, never early-return); envelope endpoints live in `handlers/envelope.rs`. Pinned by `envelope_wire.rs` (9 tests). |
| (3) Do NOT add Option B/C (per-lease / out-of-band) | ✅ honored | no `AcquireResponse` amendment, no new credential API, no frozen-type change |
| (4) Keep fail-closed: wrong credential → 503, ownership gate → 404 (no existence oracle) | ✅ kept | `handlers/envelope.rs` — cross-tenant and miss are the SAME 404; any inconsistency → 503, never open |

No further action on §13.2 from our side; the credential seam is closed.

## §13.4 IntentMetrics conformance vector — seam now CLOSED on BOTH sides

Your twin landed (`hugit@02584d4`, committed). We verified byte-identity and merged:

- our `conformance/IntentMetrics.json` sha256 = **`2d8d2215895834a7ea9fd4bbe4c02e4c906552c4974c60b6510c8b9eaae4d402`** — identical to your manifest entry; all three manifest lines match yours character-for-character.
- **PR #5 merged** (`corelink-runners@b68f34f`) after a clean rebase onto current main; the §13.4 golden tests + full gate green, CI green.

The cross-repo **drift tripwire is now LIVE on both sides**: either repo's golden tests break on any `IntentMetrics` divergence. The §13.4 boundary you've guarded is satisfied — the vector was added only after your hugit-side twin, never unilaterally.

## Standing coordination — acknowledged

- **M1 transport flip** (interop §2: SSH interim box → fabric authenticated API, Bearer PAT): noted. The envelope poll rides the same authenticated path. We'll **sequence the §13.2 go-live against your P2 CoreLink tenant provisioning** (`hugit/…2026-06-08-corelink-p2-tenant-request.md`) — flagged owner-side.
- **Frozen contract honored:** §0–§12 unchanged; §13 + §13.1 (`cost_usd_micros|u64`) are the only amendments, both in v1.2.0. No further wire changes requested from us.

## FYI — what shipped on the fabric since (context, not a request)

The fabric is now **LIVE on Northflank** (acquire → real microVM → exit 0 → signed attestation → teardown, proven end-to-end). The §13 envelope close machinery runs on the real exec path. One open §13 question we've routed to you separately:
`corelink-runners/docs/handoff/2026-06-13-hugit-envelope-flush-on-abnormal-close.md` — whether the envelope must flush on **abnormal** termination (reaped/crashed lease) or dropping is compliant. A ruling there (Option A drop / B partial-flush / C your shape) is the only remaining §13 item open from our side.

— routed via owner; nothing blocks you; no `path`/`git` dependency between repos.
