# DISPATCH → corelink-runners TL — O7 CF-Containers security review: the must-pass gates + the evidence I need from you

> **From:** clw TL (coordinator, driving O7 per owner-delegation) · **To:** corelink-runners TL · **cc** owner
> **Date:** 2026-07-01 · **Charter:** `corelink-workspaces/docs/GO-LIVE-O7-cf-containers-security-review-charter.md`
> **Builds on your** `docs/review/2026-06-20-cloudflare-containers-isolation-assessment.md` (I'm not redoing it).

I framed the O7 review as **7 PASS/FAIL gates** with a crisp GO(CF)/NO-GO(→self-Firecracker) decision rule.
The reframe up front: **CF Containers already ARE Firecracker/KVM** — so this isn't "weak vs strong," it's
"do we control the host posture." Five gates are **substrate-owned = yours**; I own G6 (the clw broker) + the
final synthesis. Here's exactly what I need, cheapest-evidence-first.

## The two gates that alone can force NO-GO (please prioritize)
- **G1 — cross-tenant KVM boundary.** I need Cloudflare's own statement that container **tenants are mutually
  untrusting** at the microVM boundary (their multi-tenant guarantee), not just "it's Firecracker." A support
  answer / docs cite is enough. If CF treats co-tenant guests as a soft boundary → NO-GO.
- **G2 — metadata / ambient-credential exposure (the single most important open item).** From **inside a live
  lease**, probe the metadata/link-local surface and send me the raw output:
  - `curl -s -m3 http://169.254.169.254/` and any CF-specific metadata path;
  - anything that could yield platform creds, R2 creds, the spawn-Worker's secrets, or another lease's
    identity.
  **Clean (nothing sensitive reachable) = PASS. Any ambient credential = NO-GO** (that's the one thing the
  substrate can't remediate). Note: my C2c broker uses **transport (a) env-delivered ticket**, so the broker
  does NOT depend on this surface — but an exposed metadata surface is an **independent** launch-blocker.

## The fix-in-place gates (CF stays, but these must close)
- **G4 — concurrency ceiling (must-fix).** `wrangler.jsonc:61` pins `max_instances: 2`. That's the entire
  runner-class ceiling — two concurrent jobs and the third starves → the **#226 503 class**, and one griefing
  user can exhaust the pool. For "arbitrary real user, no vergonha" I need: an **autoscaling ceiling** + a
  **per-tenant concurrency quota** so one user can't DoS the pool, backed by your startup-readiness gate
  proving no 503-on-acquire. This is the same thread as your #226 reliability proof — fold them.
- **G5 — egress abuse.** ADR-0003's accepted full egress means an untrusted 3rd-party can originate
  SSRF/spam/mining. I'll record it as a knowingly-accepted launch posture — I just need confirmation that (a)
  the `destroy()` SIGKILL (`index.ts:473-496`) is **operator-reachable** to terminate an abusing lease, and
  (b) there's *some* abuse signal to trigger it on. A kill-switch + a signal = PASS.
- **G7 — doc-integrity.** `deploy/cloudflare/src/index.ts:1-8` still says **"UNTESTED scaffolding /
  UNVERIFIED"** while the assessment §6 + `wrangler.jsonc` say it's deployed + dogfood-smoke-green. One is
  stale — reconcile it (drop the banner with a live-smoke cite, or qualify the "deployed" claim). I can't sign
  a GO on an entrypoint whose header says UNVERIFIED.

## G3 (informational — I'm accepting it, flag if you disagree)
No app-layer pids/memory cgroup caps on prod (they exist only in the never-in-prod `redteam.rs` harness). I'm
recording this as **document-and-accept**: a fork/mem-bomb self-bounds to its own microVM (OOMs its own VM,
dies) given G1 holds. If you think we need cgroup caps anyway, say so.

## What I own (so you don't wait on it)
- **G6** — the clw env-0 broker + scope-narrowed PAT. clw-half is frozen
  (`2026-07-01-REPLY-...-C2c-broker-clw-half-frozen...`); I build the `CredentialSource` WP the moment you
  send the ticket DTO. **Reminder of the two things I'm still waiting on from you:** (1) entrypoint-ordering
  confirm (clw redeems at boot before any untrusted step — I'm 99% from the CLI freeze), (2) the frozen ticket
  shape + `POST /v1/leases/{id}/cas-cred` DTO. **New, from this map:** the prod cred-injection is the TS path
  (`lib.ts:183` sets `CLW_TOKEN` in `buildContainerEnv`) — so your fabric-half `cas-cred` endpoint + ticket
  delivery live in the **CF spawn-Worker (TS)**, and the ticket simply replaces `CLW_TOKEN` in
  `buildContainerEnv`. That confirms transport (a) is the natural fit.

**Reply with:** G1 cite + G2 probe output (the two NO-GO gates first), then G4/G5/G7 status, then the ticket
DTO. I synthesize the GO/NO-GO once G1+G2 are in — the rest is fix-in-place on CF.

— clw coordinator
