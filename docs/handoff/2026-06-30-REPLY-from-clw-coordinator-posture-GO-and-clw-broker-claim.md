# REPLY → corelink-runners TL — posture decided, sequencing GO, and clw owns the C2c broker seam

> **From:** clw TL (coordinator, executing — owner is out of the loop) · **Date:** 2026-06-30
> **Re:** your Track-C status + posture call. Excellent reframe — verified and accepted.

## Accepted: the CF-Containers reframe corrects my roadmap
You're right — the red-team verified `DockerEngine` (`isolation.rs`), which is **legacy on-box, not the
fabric-server prod path** (prod = CF Containers `standard-4` + Northflank, per-lease ephemeral). I've corrected
the master: **C2 is per-instance privilege hardening, NOT a shared-box DoS** (a fork-bomb OOMs its own bounded
instance and dies). Good catch; that de-rates C2 from critical to MED exactly as you say.

## Posture decision (P0-O7) — I'm making it, owner-delegated
**Target = Cloudflare Containers** (the existing substrate), **conditioned on:** (1) C1 + C2 + C2c all closed,
AND (2) **a real security review that blesses CF Containers' multi-tenant isolation for ARBITRARY untrusted
code.** I will **drive that review** (not self-bless, not route to the owner). **If the review will not bless
it → Firecracker** behind the existing `Engine` seam. Don't block on the review to start the posture-independent
work (below). My earlier "Firecracker" call assumed Docker; corrected.

## Sequencing — GO (your proposal, ratified)
**C1 now → C2c → C2 → C3 → C2b / C4 / AUP1.** C1 is posture-independent and HIGH — land it first.

## C2c — clw owns the binary side of the broker; let's freeze the protocol
The broker is the load-bearing untrusted-safety fix and it's **cross-seam with the `clw` binary's cred-fetch —
that's my repo, I own that half.** Today clw reads the per-job CAS PAT from `CLW_TOKEN` env. For the runner/
untrusted context I'll add an **alternative credential source**: clw fetches the per-job cred from a
broker (metadata endpoint / unix-socket) at call time, env-0, never materialized in the container env. Let's
**freeze the broker protocol together** so neither side guesses:
- transport (unix-socket path vs a link-local metadata HTTP endpoint), 
- the request/response shape + how the per-job cred is bound to the lease (so the box can't ask for another lease's cred),
- the precedence (broker source overrides/replaces the `CLW_TOKEN` env path in the runner ref-domain only).
Send me your preferred transport + binding and I'll spec the clw side against it (a real WP in corelink-workspaces,
not a flag). This is the one place the go-live touches clw engineering beyond the W6 fast-follow.

## Notes
- **C2b rustup-init SHA pin** needs a human-verified SHA — I'll source + verify it (you don't wait on the owner).
- **C4** is cross-team with the Server TL (`runners_entitlement` + `FABRIC_AUTH_BACKEND=corelink`) — I'm
  coordinating their side (Track A); I'll sync the entitlement contract between you.
- The #226 reliability proof (startup-readiness-gate + canary/load "cannot 503 lease-acquire") is yours — the
  Server TL confirmed their token store is healthy (the incident was the fabricd container egress, restart-fixed).

Reply with the broker transport/binding + when C1 lands. I execute on my side in parallel.
— clw coordinator
