# DISPATCH → corelink-runners TL — Go-Live Track C (untrusted-code safety — HIGHEST RISK)

> **From:** clw TL (cross-team go-live coordinator) · **To:** corelink-runners TL · **Date:** 2026-06-30
> **Master:** `corelink-workspaces/docs/GO-LIVE-STACK-ROADMAP-2026-06-30.md` (dual-audited).
> Reply in this `docs/handoff/` folder; I sweep it.

## ⛔ Scope (owner override, 2026-06-30) — NO tradeoffs / gambiarras
The user is an **arbitrary real user running UNTRUSTED arbitrary 3rd-party code** — we do NOT know what they run.
The isolation must **genuinely contain it**. Every "accept / for-trusted / FC-era-follow-up" deferral is **dead**.
A red-team audit (2026-06-30) found three of these are not what the prior plan assumed — details inline.

## Your P0 track
1. **C1 — RunnerScope → tenant binding + `repo_allowlist`.** `runner_scope_from_dto` (`leases.rs`) carries NO
   tenant binding — a valid-PAT tenant can mint a JIT runner + CAS creds on any repo the shared GitHub App is on.
   Latent today, **LIVE the moment a 2nd real tenant exists** (i.e. now). Exit: scope validated against the
   caller's tenant; cross-tenant probe denied.
2. **C2 — APPLY container resource limits in the PROD spawn path.** ⚠️ Red-team: `DockerEngine::spawn`
   (`isolation.rs:128`) runs ONLY `--network none --tmpfs` — **NO `--memory/--cpus/--pids-limit/--cap-drop/
   --security-opt no-new-privileges/--read-only/--user`.** Those flags live ONLY in `redteam.rs` (a test harness,
   not wired into spawn). Untrusted code can fork-bomb / crypto-mine / OOM the shared box today. Exit: limits +
   hardening flags applied by `spawn`; 12 GiB ceiling validated vs heaviest jobs.
3. **C2c — broker the per-job credential (NEVER in the container env).** ⚠️ Red-team: the **read-write** CAS PAT
   is injected into the untrusted container's env (`runner_inject.rs:67`) with **egress on** (`:121`); untrusted
   code reads its env, exfils the PAT, and reads any digest + **poisons any AC/CAS in that tenant**
   (`runner_cas_mint.rs:691` marks intra-tenant poison "accepted by design"). Under no-tradeoffs the "FC-era
   follow-up" deferral is **pulled forward**: deliver the credential via a unix-socket/metadata **broker**, env-0,
   so an arbitrary user has nothing to exfil. Exit: no secret in the container env; exfil test yields nothing.
4. **C2b — exec-server auth + rustup pin + isolation posture.** Add auth on the check-exec-server (`lib.rs:7-9`,
   no auth today) as defense-in-depth; pin rustup-init SHA; and the **owner posture decision (P0-O7)**: a real
   security review must bless hardened Docker for **arbitrary untrusted** code, **or escalate to Firecracker**.
   Not "Docker is fine for a trusted user."
5. **C3 — arm the vCPU-h ceiling** (`FABRIC_RUNNER_VCPU>0`) + fix the 2 billing-robustness mediums (durable
   open-lease map, accounting-on stale-Pending sweep) so the loss-impossible guarantee is real.
6. **C4 — flip auth to CoreLink** (`FABRIC_AUTH_BACKEND=corelink`) + `runners_entitlement` lookup + the tenant row.
7. **AUP1 [P1, still gating] — an enforcement primitive** (tenant-suspend / lease-kill / forensic trail) so the
   AUP is enforceable if the untrusted user runs abusive/illegal workloads.

## What I need back
Per-item status + your call (with the owner) on **C2b posture: hardened-Docker-blessed vs Firecracker**, and the
honest effort on C2c (the credential broker) — it's the load-bearing untrusted-safety fix.
