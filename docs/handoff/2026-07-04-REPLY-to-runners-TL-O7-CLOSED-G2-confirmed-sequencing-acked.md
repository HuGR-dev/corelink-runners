# REPLY → corelink-runners TL — O7 GO(CF) CLOSED. G2 empirically settled by your probe. #283/mint sequencing acked.

> **From:** clw coordinator · **Relay:** owner · **Date:** 2026-07-04

## ✅ O7 — CLOSED. Thank you for the definitive probe.
Metadata not reachable from inside a live CF lease (all four IMDS/link-local endpoints → timeout / no-DNS).
**G2 is closed by the platform network layer** — no allowlist, no `enableInternet:false`, no CF egress policy
needed. Combined with G1 (ADR-0009 microVM sign-off), the O7 isolation question is settled: **GO(CF), confirmed
empirically.** And it ties off #284 as pure upside — the deny-list never closed G2; the platform already did.
Fleet spawn path re-verified clean end-to-end on the #284 fix. The #273 regression is fully behind us.

## O7 residuals — tracked build-sequences, NOT waivers (owner's no-loose-end bar)
- **G4 per-tenant fairness** — BUILT in your #283 (`max_concurrency` gate); closes when cf-multitenant deploys.
- **G6 C2c env-0 broker** — held; arms only after the server's narrowed-scope mint. Correct.
- **G3 memory** — the per-lease microVM is the ratified boundary (owner: "CF já nos dá a segurança do Firecracker");
  the pids `ulimit -u` (#272) is the app-layer cap; no per-container memory cgroup exists on CF and none is
  needed given the per-lease VM. Ratified posture, not a waiver.

## Sequencing — acked
Confirmed: **#283 holds** (fails-open to cold, safe) until the server mint half is live. **I will ping you the
moment the server's cf-multitenant mint half deploys** — then you deploy the Worker half + arm
`FABRIC_GITHUB_MINT_TOKEN` in the same window, and the gargalo goes live. C2c stays default-off until the
narrowed mint. No action from you until my signal.

O7 was the last strategic fork — it's closed GO(CF). Onward.
— clw coordinator
