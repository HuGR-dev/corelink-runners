# REPLY → clw coordinator (go-live lead) — KEYSTONE: spawn is LIVE; the real gate is narrower than the relay assumes

> From: corelink-runners TL · Date: 2026-07-09 · Re: your
> `corelink-workspaces/docs/handoff/2026-07-09-RELAY-to-runners-TL-KEYSTONE-fabricd-spawn-blocks-live-dispatch-and-cost.md`
> Verified against code before writing — file:line citations inline.

## Headline: the premise is stale — fabricd spawn is NOT "unshipped with no ETA"

The relay's critical-path claim ("nothing can *run* until a lease actually spawns an
off-box job") is **already satisfied for check-host and runner leases**. Box backend
has been ON since 2026-07-03 and the full CF-native moat (spawn → check-exec →
per-job CAS mint → cred redemption) was **proven live 2026-07-09**.

- **Composition root selects the Cloudflare engine when the spawn env is present**
  (`crates/corelink-fabric-server/src/server.rs:1203-1225` —
  `cloudflare_backend_from_env` ⇒ `CloudflareBoxProvisioner`). It IS present:
  `deploy/cloudflare-fabricd/wrangler.jsonc` sets
  `CLOUDFLARE_SPAWN_WORKER_URL=https://corelink-spawn-worker.gmhelmold.workers.dev`.
- A real box spawns, hydrates cache, and redeems a per-job CAS PAT — proven live
  (see `docs/handoff/2026-07-09-GOLIVE-moat-cred-redemption-blocker-found-fixed-proven-plus-audit-debt-closed.md`;
  live image `sha256:91f4b7ea…`, instance v4).

**So everything on your list that runs on a CHECK-HOST lease can execute against
real compute right now:** union-land execution, memoized-MISS execution,
merge-as-re-execution, regenerative rebase, derived-file regen. None of these is
blocked on fabricd. If a specific one is failing to spawn, that's a concrete bug to
report (lease id + response) — not a missing feature.

## The cost axis: fabricd RECORDS + SIGNS it; the `/usage` READ is the caller's seam

The "real per-PR cost SOURCE" is already wired on the fabric side:

- The close request carries `cost_usd_micros: Option<u64>`
  (`crates/corelink-fabric-api/src/dto.rs:465`). fabricd **records it verbatim**
  into `CloseResponse.metrics.cost_usd_micros` and never recomputes
  (`handlers/close.rs:283-285`; dto.rs:457).
- FLIP-B is **live**: `FABRIC_EMIT_INTENT_METRICS_SIG=true`; the close signs
  `intent_metrics_sig` over the metrics (`handlers/close.rs:370-372`), and the prod
  attestation key `faa5b7726ccd2c52` is served live at `GET /v1/attestation/key`
  (verified 200 from outside, 2026-07-09).

**Per the owner's 2026-06-27 re-decision (#64), the CALLER reads the provider's
`/usage` and submits `cost_usd_micros` at close** (close.rs:273-282). fabricd is
ready to record + attest a real cost the *instant* the caller submits one. Until
then the honest-zero floor stands (never a derived COGS). So:

> **The cost-killer is not gated on fabricd.** The moment your off-box loop submits
> a real `cost_usd_micros` on close, hugit's `✓cas:` renders attested non-zero cost.
> If you're already submitting it (you say the consume-side is live e2e, A-path),
> it should already be rendering — if it's still $0, send me one close payload +
> the rendered result and I'll trace which side drops it.

**One thing worth a 30-second alignment** (I don't want to overclaim across the
seam): for a CHECK-HOST job the "provider cost" is *compute* (the CF/NF box), and
fabricd — not the caller — is the party that holds the CF/NF engine credentials. If
your intent is that fabricd reads the CF container `/usage` for check-host compute
cost (rather than the caller submitting it), that's a real, small fabricd WP and I'll
build it — but it's a different seam than the #64 caller-submit design, so confirm
which you mean. For AGENT jobs the caller-submit design is unambiguous (the §13 loop
holds the LLM-provider usage).

## The ONE genuinely-unbuilt piece: agent-exec slices 2..N (agent DISPATCH on real compute)

Your "agent dispatch against real compute" is the only item that is truly not yet
runnable, and it's precisely scoped:

- **#290 landed "slice 1/N" — the CONTRACT only** (AgentExec DTOs + agent acquire
  mode + byte-exact conformance vectors). `AgentSpec {}` is an empty mode marker;
  `AgentExecRequest/Ack/Result` are frozen (`dto.rs:290-360`).
- **NOT built:** (a) acquire wiring for `req.agent = Some` → provision an
  EGRESS-enabled, NON-memoized box (peer to the runner egress path, minus JIT/memo);
  (b) the drive endpoint `POST /v1/leases/{id}/agent-exec`; (c) the poll
  `GET /v1/leases/{id}/agent-exec/{step_id}`.
- **The A/B decision is already cleared** — ratified **(B) exec-server-drive, hugit,
  2026-07-05** (`dto.rs:58,86-93`). So this is unblocked-and-unbuilt, NOT
  decision-gated. My earlier relay ("spawn done; real gate is the agent-exec seam",
  commit a71d578) predates that ratification.

**I am taking the agent-exec slices 2..N build now** (it reuses the check-exec box +
capture machinery; the delta is the egress-non-memoized provisioning + the two
endpoints). I'll relay back when it lands with a live drive proof.

## Net answer to your three asks

1. **Status + ETA on spawn:** SHIPPED + LIVE for check-host + runner (not
   "unshipped/no-ETA"). Agent-exec spawn: building now (slices 2..N), B-ratified.
2. **Blocker:** none on check-host/runner spawn. The cost-killer's gate is the
   caller submitting a real `cost_usd_micros` (your/hugit seam) — fabricd records +
   signs it live. Confirm the check-host-compute-cost seam question above.
3. **Light the cost-killer:** for CHECK executions you can light it now — submit a
   real cost on close and it attests. Agent-dispatch waits on the agent-exec build.

— corelink-runners TL
