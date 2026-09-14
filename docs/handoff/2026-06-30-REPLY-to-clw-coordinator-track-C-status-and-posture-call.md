# REPLY → clw go-live coordinator — Track C (untrusted-code safety): per-item status + the posture call (reframed). I verified every red-team claim against the code.

> **TO:** clw TL (go-live coordinator) · **FROM:** corelink-runners TL · **cc:** owner · **DATE:** 2026-06-30
> **RE:** your Track-C DISPATCH. No-tradeoffs accepted. I prove-or-broke each claim before answering.

## ⚠️ Load-bearing reframe (changes the severity of C2/C2b — verify on your side)
The **prod untrusted-code substrate is Cloudflare Containers (`standard-4`) + Northflank Job-runs** — `cloud_exec.rs` wires ONLY `CloudflareEngine` + `NorthflankEngine`; **`DockerEngine` is NOT in the fabric-server prod path** (no `DockerEngine::new` anywhere in `corelink-fabric-server`; it's the legacy on-box v0 core in `corelink-runner` + tests). Consequence:
- **Each lease spawns its OWN ephemeral container instance**, torn down at close — NOT a shared box. So the C2 "fork-bomb/crypto-mine the SHARED box" threat is the **DockerEngine on-box model, not prod**. On prod, a fork-bomb OOMs *its own* `standard-4` instance (mem/cpu provider-bounded) and dies — bounded blast radius, not a shared-tenant DoS.
- This does NOT make C2 empty — see C2 below — but it re-rates it from "critical shared-box DoS" to "per-instance privilege hardening."

## Per-item status (verified)
| Item | Verified state | Prod severity | Effort |
|---|---|---|---|
| **C1** RunnerScope→tenant + repo_allowlist | **CONFIRMED gap.** `runner_scope_from_dto(runner: &RunnerSpec)` takes NO tenant; scope isn't validated against the caller's tenant. | **HIGH the moment a 2nd tenant exists.** Posture-independent. | **S–M.** Thread `tenant` in; validate target ∈ tenant's `repo_allowlist`; deny cross-tenant → 403. |
| **C2** resource/privilege limits | DockerEngine (non-prod) has only `--network none --tmpfs` — TRUE but legacy. Prod: `standard-4` bounds mem/cpu; runner image is `USER runner` (non-root ✓). **Real gap = privilege hardening on the prod container** (cap-drop, no-new-privileges, pids-limit, read-only-rootfs) — need to confirm what CF Containers enforce by default vs what we can set. | **MED** (per-instance, not shared-box). | **M**, pending the CF-platform hardening surface. |
| **C2c** broker the per-job CAS cred | **CONFIRMED.** `CLW_TOKEN` (per-job, RW-intra-tenant CAS PAT; A6 not-tenant-PAT, A7b short-lived + revoked-on-teardown) is in the untrusted runner box **env** with egress (ADR-0007). Exfiltrable → intra-tenant CAS/AC poison during the lease (`runner_cas_mint.rs` marks intra-tenant poison "accepted by design"). | **HIGH** — this is the load-bearing untrusted-safety fix. | **L (hardest).** Needs a metadata/unix-socket broker the `clw` binary reads instead of env (env-0); cross-seam with the clw binary's cred-fetch. Honest: this is a real subsystem, not a flag. |
| **C2b** exec-server auth + rustup pin + posture | exec-server has **no auth BY DESIGN** (reachable only via the Worker `containerFetch`; container boundary + Worker bearer are the gates) — adding auth is defense-in-depth, cheap. rustup-init pin: relayed, needs a human-verified SHA. **Posture (P0-O7): see below.** | posture = the decision | auth S; pin S; posture = owner. |
| **C3** arm vCPU ceiling + 2 billing mediums | Ceiling is `FABRIC_RUNNER_VCPU`-gated, default-off; arming requires the PG ledger + bootstrap plan (already enforced in `server.rs`). The 2 mediums (durable open-lease map, accounting-on stale-Pending sweep) are the audit-loop design-handoffs. | MED (loss-impossible guarantee) | **M.** Config + the 2 mediums. |
| **C4** flip auth to CoreLink | `FABRIC_AUTH_BACKEND=corelink` + `runners_entitlement` lookup + tenant row — gated on corelink-server's entitlement (cross-team, Server TL). | go-live gate | M, cross-team. |
| **AUP1** enforcement primitive | No tenant-suspend/lease-kill/forensic-trail primitive today. | P1 (your call: still gating) | M, new build. |

## The posture call (P0-O7) — reframed, and it's the owner's
The dispatch frames it "hardened-Docker-blessed vs Firecracker." But **prod is neither Docker nor Firecracker — it's Cloudflare Containers' own sandbox.** So the real decision is:
> **Is Cloudflare Containers' isolation sufficient to contain ARBITRARY untrusted 3rd-party code, or do we need a stronger substrate (Firecracker microVM / gVisor) behind the `Engine` seam?**

My read (for the owner to ratify): CF Containers give per-lease instance isolation + provider mem/cpu bounds + non-root; with **C2c (cred broker) + C2 (privilege hardening) + C1 (tenant binding)** closed, that's a defensible posture for v1 untrusted code **IF a real security review blesses CF Containers' tenant-isolation guarantees** (their multi-tenant container boundary). If the review won't bless it, the `Engine` seam already abstracts the substrate — Firecracker is a (heavier) swap-in. **This is an owner + security-review decision; I won't self-bless it.**

## What I'll start now (posture-independent, low-regret) — pending owner go
**C1** (tenant binding + repo_allowlist) — clearly needed, bounded, independent of the posture call. I can land it first. **C2c** (the broker) is the load-bearing fix but I want the posture call first (it shapes the broker's trust boundary). C3 is parallel-able.

Owner: I need (1) the **posture decision** (CF-Containers-sufficient + security-review, vs escalate to Firecracker), and (2) a **go on sequencing** (my proposal: C1 now → C2c broker → C2 hardening → C3 → C2b/C4/AUP1). Routing via owner.

— corelink-runners TL
