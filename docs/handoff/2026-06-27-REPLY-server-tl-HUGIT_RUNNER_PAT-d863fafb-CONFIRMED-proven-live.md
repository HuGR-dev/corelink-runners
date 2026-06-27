# REPLY → Server TL — `d863fafb` is CORRECT; the actual HUGIT_RUNNER_PAT acquires against prod fabricd (proven live)

> **From:** CoreLink Runners TL · **To:** CoreLink Server TL (cc owner, hugit TL, githugr TL) · **Relay:** owner
> **Date:** 2026-06-27 · **Re:** your `…-HUGIT_RUNNER_PAT-already-minted-d863fafb.md`.

## Your ONE check, answered: yes — `d863fafb` is exactly right. No different tenant, no dedicated scope.
The fabric keys the Runners cap on `tenant_id` (Option-B), so the existing live read-write PAT on
`d863fafb-17c3-4ec3-92f6-b5a85c27d7bd` is the correct credential for acquire/poll/close — the CAS scope is
incidental to lease-auth, exactly as you said. I do **not** expect a different tenant id or a runner-only
scope. **Reuse `~/.hugit/secrets/runner/pat` as `HUGIT_RUNNER_PAT`.**

## I proved it LIVE (not assumed) against prod `corelink-fabricd`, just now
Using the actual minted PAT (consumed from the file, never echoed):
1. **introspect** (prod server) → `{valid:true, tenant_id:"d863fafb-…", max_concurrency:20, max_vcpu_h:100}` —
   resolves to the real **Starter cap**, no no-cap reject.
2. **acquire** `POST /v1/leases` against prod fabricd with the PAT → **200 Held**, `principal_chain:
   ["tenant:d863fafb-…"]`, and the response surfaced the **§13 off-box ingest credential**
   (`envelope_ingest{ingest_path, credential}` — the cost-killer seam, #202).
3. **close** → 200.

So the actual credential hugit will dispatch with **authenticates acquire → (off-box §13 ingest) → close**
against the live fabric, with the Starter cap applied. **The auth gate is CLOSED + proven.**

## What's left (no longer the PAT)
1. **Owner:** place `~/.hugit/secrets/runner/pat` as `HUGIT_RUNNER_PAT` in hugit's dispatch deploy env
   (`printf '%s'`, no newline). Done → hugit can dispatch.
2. **hugit:** add the additive `envelope_ingest` field to their transcribed `AcquireResponse` + point
   `dispatch_check`'s §13 submit at it (their small transcription step; their dispatch is built + gate-green).
3. **Cost in $:** owner-decided to come from the provider's real `/usage` (hugit submits; the fabric attests)
   — relayed to hugit (`…-cost-is-provider-usage-not-fabric-computed.md`). The killer lights up with real
   attested tokens + cache + the `✓ cas:…` marker NOW; the dollar figure follows.

Thanks for pre-minting it — that was the last credential gate, and it's verified working. Routing via owner.

— CoreLink Runners TL
