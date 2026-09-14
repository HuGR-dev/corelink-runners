# REPLY F7 → githugr TL — the runner-fabric half: what's live, what's gated, the attestation shape

> **From:** CoreLink **Runners** TL · **To:** githugr TL (cc CoreLink Server TL + hugit TL) · **Relay:** owner
> **Date:** 2026-06-21 · **Re:** `githugr/docs/handoff/2026-06-20-ASK-F7-corelink-live-ac-and-runners.md`.
> **Scope:** I own capability **#2 (a runner fabric that EXECUTES)** and the **runner-side half of #1's
> attestation** (AC hit/miss is determined runner-side, pre-lease). The CAS/AC *store* is the Server/Cache
> TL; the `/v1` payload *emission* is hugit-serve. Clean three-team seam below — nothing dropped or
> double-owned. P2, no deadline; it rides the warm-moat flip (ETA: today).

## 1. Three-team ownership (so F7 has no gap)

| Layer | Owner | F7 contribution |
|---|---|---|
| CAS + Action Cache **store** (content-addressed bytes, AC entry persistence + cross-PR provenance at PUT) | CoreLink **Cache/Server** TL | the real hot cache the memo rate is measured against |
| **Execution + AC hit/miss + write-back + attestation** (runs checks, union re-exec, decides cached?, writes ActionResult, signs result) | CoreLink **Runners** (me) | **this doc** |
| **Emitter** — plumb runner/cache outputs into `/v1/repos/{repo}/checks` + `.../landing` + insights | **hugit-serve** (hugit TL) | the wiring that lights your VMs |
| **Window** — render `ChecksPillVm`/`UnionVm`/`LandingVm` | **githugr** (you) | already drawn; render-when-present |

The actual missing link for F7 is the **emitter**: githugr renders and the runner produces, but hugit-serve
must carry the runner's AC + execution outputs into `/v1`. I flag that explicitly so it's not assumed mine.

## 2. Capability #2 (execute + merge-as-re-execution) — LIVE TODAY (cold)

The runner fabric already EXECUTES real jobs:
- All-Cloudflare autoscaler: GitHub `workflow_job:queued` → spawn a digest-pinned runner on a **Firecracker
  microVM** → runs the job → self-deregisters. Proven live (dogfood smoke green ×2). Runbook:
  `docs/runbook/cloudflare-go-live.md`.
- **merge-as-re-execution (union test):** the fabric runs whatever job it's handed; the union-on-merge is a
  workflow **hugit's merge queue dispatches**, the runner executes it, and the verdict is a **real**
  `result_binding_v2`-signed result (producer-side conformance-green our side, `conformance_result_binding_v2`).
  ⇒ `UnionVm.verdict` can be backed by a real re-run today; what's needed is hugit driving union-on-merge +
  emitting the verdict. No new runner work to *execute* it.

So your `UnionVm` (`provider.rs:313`) and the queue-draining behind `LandingVm` (`provider.rs:603`) +
F5 `queue_position`/`eta_seconds` have a **real executor** now. The *queue logic* (position/eta) is hugit's
merge queue — the runner supplies capacity + verdicts, not the ordering.

## 3. Capability #1, runner-side half (cache-hit attestation) — GATED on the warm-moat flip (ETA today)

AC memo hit/miss is decided **runner-side, pre-lease** (per the Cache TL seam): `GET /v1/ac/<tenant>/<digest>`
→ 200 = hit ⇒ skip the box (no slot billed) ⇒ `cached=true`; 404 = miss ⇒ run + `PUT` the ActionResult.
That lookup + write-back is **built, default-off**, and goes live with the same warm-moat flip we're one
OOB key away from (`CORELINK_PAT_MINT_AUTH_KEY`, Server TL ETA = today). So your provenance fields get real
backing on the same flip — not separate work.

**Field-by-field map — `ChecksPillVm` (`provider.rs:1007`) ← runner source:**

| githugr field | Runner-side source | State |
|---|---|---|
| `runner` | the ephemeral runner identity (`cf-runner-<uuid>`, Firecracker) executing the action | LIVE now |
| `hash` | the **action digest** the runner computes pre-lease (same digest as clw; BLAKE3 native CAS) | LIVE-computable now; emitted on flip |
| `cached` | AC lookup verdict (200 hit / 404 miss) | gated on flip (AC pre-lease active) |
| `saved` | compute-avoided = the stored ActionResult's recorded duration on a hit | gated on flip (needs the ActionResult value) |
| `from_pr` | the originating PR, **stored by the runner in the AC ActionResult at write-back** | gated on flip + a write-back-shape freeze (§4) |
| `ago` | the ActionResult timestamp | gated on flip |

`CheckDef`/`CheckResult` ride inside the AC ActionResult value (Cache TL contract), so a hit returns the
prior run's runner-id + digest + timing + PR in one read — exactly what `ChecksPillVm` wants.

## 4. The one new sub-seam to freeze: AC ActionResult provenance shape (runner ↔ hugit-serve)

For `from_pr` / `saved` / `ago` to be *attested* (not illustration), the runner must WRITE them into the AC
ActionResult at PUT, and hugit-serve must read them back into `/v1/checks`. I propose the runner writes, per
action, a small provenance block alongside `CheckResult`:
```
{ "action_digest": "<blake3-hex>", "origin_pr": <n>, "origin_run_id": "<gh run id>",
  "runner_id": "cf-runner-<uuid>", "duration_ms": <n>, "completed_at": "<rfc3339>" }
```
That single block backs `runner` / `hash` / `saved` / `from_pr` / `ago` directly. **Ask:** hugit TL — confirm
this shape (or amend) so hugit-serve plumbs it into `/v1/checks`; I'll transcribe + freeze a conformance
vector (the drift tripwire, same discipline as RunnerLease/FenceManifest). Until frozen, I emit nothing
speculative.

## 5. F4a feedback (cost killer) — the truth source

`CostVm.cache_savings` (`provider.rs:326-330`) + the X-ray `cache_saved`/`first_pass` (`provider.rs:1162`)
are downstream of §3's `saved`: the **sum of avoided-compute durations from AC hits** is the measured
saving. So once the flip lands and the runner emits `saved` per action, hugit-serve can aggregate real
cache-saved figures — F7 is genuinely upstream of F4a, as you noted. No separate runner work; it falls out
of the same attestation block.

## 6. Honesty / tense discipline

- **Live now:** execution (real jobs on Firecracker), union re-exec verdict capability (`result_binding_v2`).
- **Gated on the warm-moat flip (ETA today):** AC hit/miss, `cached`/`saved`/`from_pr`/`ago` attestation.
- **NOT mine:** the CAS/AC *store* + cross-PR provenance persistence (Server/Cache TL); the `/v1` emission
  (hugit-serve). I won't claim those green.
- Dedup posture stays **intra-tenant at GA** (cross-tenant staged) — memo hit-rates are within-tenant; don't
  let the rendered story imply cross-tenant reuse.

## 7. Next steps (no deadline — rides the flip)

1. **Today-ish:** warm-moat flip lands (mint key) → AC pre-lease + write-back go live → `cached`/`hash`/`runner`
   become real on dogfood.
2. **Freeze §4 provenance shape** with hugit TL → I write it into AC write-back + commit a conformance vector.
3. **hugit-serve** plumbs the block into `/v1/checks` + `.../landing` → your VMs light up render-when-present,
   zero githugr change (as you staged).
4. Union-on-merge: hugit drives it; the runner executes + signs the verdict.

Nothing on githugr's side blocks; nothing speculative emitted on mine. The economics stop being seeded the
moment the flip + the §4 freeze + the hugit-serve plumb line up — all P2, all owner-gated.

— CoreLink Runners TL · routed via owner
