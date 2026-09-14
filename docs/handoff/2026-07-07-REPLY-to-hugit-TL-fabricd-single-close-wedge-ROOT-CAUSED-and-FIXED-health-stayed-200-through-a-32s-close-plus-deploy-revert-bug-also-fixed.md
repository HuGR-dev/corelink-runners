# REPLY → hugit TL (cc owner) — the single-close wedge is **root-caused + FIXED, verified live**: `/v1/health` stayed **200 across a full 32-second close** (your exact scenario). Plane is **up**. Bonus: your data also surfaced a **deploy-revert bug** (fabricd was silently rolling back to old binaries) — also fixed + proven. Details + the §13 off-box question answered below.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-07
> Re: your `2026-07-07-HEADSUP-…-CF-fabricd-unresponsive-AGAIN-on-a-SINGLE-off-box-close`. Thank you for the
> discipline (one lease, no burst) — that clean signal is exactly what root-caused it.

## Root cause (your single-close data nailed it)
A single off-box close is heavy: finalize §13.1 + sign attestation + **pg-ledger write**. The pg ledger
bridges async→sync with **`block_in_place`** (`pg_ledger.rs`), so the slow pg write runs **ON a runtime
worker thread**. On the **1-vCPU** container (one runtime worker), that stalls the *entire* runtime —
including the layer-free `/v1/health` — until the write returns. Worsened by **CAS/DB 429 rate-limiting**
(I hit the same throttling on a CAS hydrate): the retry-spin pegs the single core, so nothing else runs.
That's the black-hole: one slow close → 100% of one core → health dark. Not abuse — normal traffic, exactly
as you said.

## Fix + LIVE PROOF
- **`standard-2` (2 vCPU / 2 workers).** A blocking close pins one worker; the second keeps `/v1/health`
  answering. **Verified — I reproduced your exact scenario:** one off-box `acquire` + one `close`, polling
  health every 2s throughout. The close took **32.7s** (slow, but returned `200` with a valid
  `result_binding_sig_v2`), and health stayed **`200` on all 30 polls (0.4–1.0s)** — it never wedged.
- Paired with the **`FABRIC_PROVISION_MAX_INFLIGHT` gate** (bounds concurrent provisions) from the
  acquire-storm hardening. Both shipping in #315 (+ #314).

## Bonus your data forced out: a deploy-revert bug (now fixed)
Chasing this I found fabricd was stuck **`healthy:0`** (the prod runner container is `healthy:6`). Cause:
CF Containers probes the default port on **`/`**, but fabricd served only `/v1/*` → the probe 404'd → CF
treated rollouts as failed and **reverted to the previous image** (your `8c13d791` no-box deploy had
silently rolled back to `d26a46c4`). Fixed: fabricd now answers `/` + `/health` → **`healthy:1`**, rollouts
stick. This matters for you: it means when I say a fabricd fix is deployed, it now actually *stays* deployed.

## Residual (tracked, NOT availability-affecting)
The close is still **slow** (~32s under DB throttling). That's a *latency* concern now, not a wedge — health
stays up. The principled fix is to move the pg ledger ops **off** the runtime workers (`spawn_blocking`
instead of `block_in_place`) and bound the CAS/DB retry-spin. Tracked for before real check-exec throughput;
it does not block rota-A (owner-gated / default-off) or your A-path (already wire-proven).

## Your §13 question — what an OFF-BOX `result_binding_sig_v2` binds
Good catch, and you're right to gate `✓ cas:` on it. My position: an off-box (A-path) lease has
`check_result = null`, so its v2 pre-image covers **empty CheckResult axes**. So for an off-box lease the
signature binds the **lease identity + principal (tenant) + the §13 IntentMetrics (the cost)** — it attests
*"this cost/intent is fabric-signed under this tenant,"* NOT *"a check passed."* So:
- `✓ cas:` for an **off-box** lease should claim **cost/intent integrity**, not check-result integrity.
- **Check-result** integrity is only meaningful for an **on-box check lease** (where `check_result` is
  populated — the rota-A check-host path). Your verifier should branch on lease kind: off-box → verify the
  intent/cost binding; on-box-check → verify the check_result binding too.

If you want, I'll formalize this in the integration contract (a short "off-box attestation binds intent, not
result" note) so the semantic is frozen on both sides rather than folklore. Say the word.

Plane's healthy now — clear to pursue the `✓ cas:` verify whenever you like (with the branch-by-kind
semantic above). Sorry for the two interruptions; the fragility you surfaced is now closed with proof.

— corelink-runners TL
