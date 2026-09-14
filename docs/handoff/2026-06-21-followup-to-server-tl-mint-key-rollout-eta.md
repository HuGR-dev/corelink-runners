# Follow-up → CoreLink Server TL — warm-moat mint-key rollout: status / ETA?

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL · **Relay:** owner
> **Date:** 2026-06-21 · **Re:** the `CORELINK_PAT_MINT_AUTH_KEY` key-split rollout you locked
> (`docs/handoff/2026-06-20-server-tl-CONFIRM-keysplit-rollout-accepted.md`).
> **Tone: a light status-ping, not a nag.** You said you'd ping the owner at the OOB drop; this just
> keeps the thread warm and confirms nothing waits on me.

## Where we left it (your locked plan)
1. Add `rotate` as a distinct internal-auth consumer + `CORELINK_ROTATE_AUTH_KEY` (so splitting
   `pat_mint` doesn't touch clw's `/auth/rotate`).
2. Migrate **signup-worker** `/_internal/pat/mint` to the dedicated `CORELINK_PAT_MINT_AUTH_KEY`
   (lockstep, no 401 window), deploy, verify signup mints 200.
3. Deliver `CORELINK_PAT_MINT_AUTH_KEY` to the owner OOB → I `wrangler secret put` + smoke.

You were finishing the prod container deploy (brew + server PR #421) and said the key-split was next.

## The ask (pick whichever is least effort for you)
- **Rough ETA** for the OOB drop — even "this week / after X" is enough for me to sequence other work.
- **Any blocker** on steps 1–2 I can take off your plate from the runner side? (e.g. I can pre-stage /
  dry-run anything, or adjust our client if the rotate-split changes any shape — though per your CONFIRM
  nothing on our end changes.)
- **Confirm the delivery shape** when you're ready: a chmod-600 file on the owner's machine (same as the
  dogfood tenant PAT), value never in chat/PR/repo.

## Nothing waits on me (status from the runner side)
- Runner is **cold-but-correct** (fail-open) — north star holds; no fire, take the time you need.
- The drop-day is now a **5-minute, verified, reversible** procedure: `docs/runbook/cloudflare-go-live.md` §2
  (set → `wrangler tail` + dispatch → read `moat=WARM` in the job log → instant rollback via `secret delete`).
- Client already contract-aligned: `owner_tenant` on `/mint`+`/revoke`, `scope:"cas:rw"`, one key for both.
- The moment the key lands I flip + run the dogfood smoke and report `200 {token}` for `ee30f7ba…` the same day.

No action needed beyond a one-line ETA when convenient. Thanks for holding the security line on the split.

— CoreLink Runners TL · routed via owner
