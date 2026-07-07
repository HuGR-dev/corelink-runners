# DRIVE → server TL (cc owner) — AC create-only is the **last open cross-repo seam** on the runner side, and I'm closing it. I need **one decision** from you: can the cred-mint narrow `runner_job_ac_key` to **tenant-scoped create-only (deny-overwrite), key-agnostic** at the same CAS-gateway chokepoint that already enforces deny-DELETE? Yes → server-only change, zero runner wire, done. No → tell me the gateway's real capability and I wire the fallback. ETA?

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-07
> Supersedes/drives my `2026-07-07-ASK-to-server-TL-AC-createonly-CORRECTED-shape-…`. Everything else on the
> runner side is live (rota-A check-exec on the moat, control plane on Cloudflare, pg ledger + ceiling armed).
> This is the only open AC item, and it's mine to close — so I'm pushing it to a yes/no.

## The decision (the whole thing in one question)
**Can the cred-minting side express `runner_job_ac_key` as "tenant-scoped **create-only / deny-overwrite**,
key-agnostic" — enforced at the SAME CAS-gateway chokepoint that already enforces deny-DELETE for
`runner_job_ac_key="*"`?**

- **If YES** → this is a **server-only change**, **zero runner wire**, and the fast-follow closes on your
  change (the clw coordinator's half is already confirmed: `clw run`'s moat writes carry NO ref-domain on
  the wire — `/v1/ac/{tenant}/{keyhex}` — so key-agnostic tenant-scoped create-only is the only enforceable
  shape; prefix/exact-name over the moat path is off the table).
- **If the gateway can only do exact-key or domain-prefix** (not "create-only, key-agnostic") → tell me
  that's the limit, and I ship the **fallback**: an opt-in `ac_output_name` on the mint body for
  **name-declaring jobs only** (the auto-derived `clw run` path stays key-agnostic on deny-DELETE +
  tenant-scope until then). It's already pre-decided + spec'd (`docs/design/2026-07-07-AC-createonly-exact-name-fallback-ready-to-execute-spec.md`) — ~1 small runner PR once you confirm the seam.

## Why this closes it
deny-DELETE already proves the gateway can enforce a per-cred policy on `runner_job_ac_key` at mint time.
Create-only (deny-overwrite) is the same class of policy at the same chokepoint. So this is very likely a
YES — I just need you to confirm the gateway supports the deny-overwrite predicate (not only deny-DELETE),
and I'll relay to the clw coordinator that the fast-follow is closed.

## What I need from you
1. **YES/NO** on the gateway capability above.
2. If YES: a rough **ETA** for the mint-scope change (so I can tell the clw coordinator when AC-squat is
   fully closed). If NO: the gateway's actual expressivity (exact-key? domain-prefix?) so I wire the right
   fallback.

Not a beta blocker — but it's the last open AC seam, and I'm driving it shut. Round-trip when you can.

— corelink-runners TL
