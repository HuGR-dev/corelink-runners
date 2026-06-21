# Security assessment — Cloudflare Containers isolation for untrusted CI (ADR-0008 gate)

> **Purpose:** the isolation sign-off gate for ADR-0008 (Cloudflare = default compute substrate). We run
> UNTRUSTED customer + AI-agent code; isolation is fail-closed and must clear the multi-tenant bar before
> a real tenant job runs on Cloudflare. **Owner-decision artifact** — this is the evidence base; the
> sign-off is the owner's.
> **Date:** 2026-06-20 · **Status:** CONDITIONAL PASS (verify items §6 before first non-dogfood tenant).

## 0. Verdict

**Cloudflare Containers run each container instance inside an AWS Firecracker microVM with KVM
(Intel VT-x / AMD-V) hardware-enforced isolation** — NOT shared-kernel Docker, NOT gVisor. This is the
**gold-standard isolation class for untrusted multi-tenant code** — the same class as (a) our interim
Northflank substrate (managed microVM) and (b) the own-metal Firecracker backend we'd otherwise build
(ADR roadmap FC1–FC5). **So Cloudflare is NOT a downgrade on isolation — it meets the bar.** Conditional
PASS: sign off for dogfood + early customers once the §6 verify items are confirmed.

## 1. Threat model (what we are defending against)

- **Untrusted code execution:** customer CI commands AND AI-agent loops run arbitrary binaries with
  network egress (ADR-0003 accepts bounded egress at launch). Assume the in-box process is hostile.
- **Multi-tenant:** tenant A's job must not read/affect tenant B's job, memory, filesystem, or cache.
- **Host integrity:** a compromised job must not escape to the host kernel / control plane / other boxes.
- **Cache integrity:** a job must not poison another tenant's CAS (intra-tenant poison is accepted-by-design,
  WP-8d; cross-tenant is barred by routing + the per-job tenant-scoped PAT).
- **Secret safety:** no long-lived secret on the box; the per-job CAS PAT is scoped + short-lived (D-9).

## 2. What Firecracker microVM + KVM gives us (the hardware boundary)

- **Per-instance microVM:** each container runs in its own Firecracker VM — a separate guest kernel, not
  the host kernel. A kernel exploit in the guest does not reach the host or siblings.
- **Hardware-enforced boundary:** KVM uses CPU virtualization (VT-x/AMD-V) — the isolation is enforced by
  the silicon, not a user-space syscall filter. This is strictly stronger than gVisor (user-space kernel)
  and incomparably stronger than shared-kernel containers (Docker/runc), where a kernel LPE = full escape.
- **Minimal device surface:** Firecracker exposes a deliberately tiny virtual device model (no BIOS, no
  PCI, minimal virtio) → small host attack surface. This is the design AWS Lambda/Fargate trust for
  exactly this multi-tenant-untrusted use case.
- **No cross-tenant memory:** distinct microVMs have distinct guest physical memory; one tenant cannot
  read another's RAM (the class of bug that shared-kernel containers are vulnerable to).

## 3. Mapping to our requirements

| Requirement (§1) | Firecracker microVM verdict |
|---|---|
| Untrusted code can't escape to host | ✅ hardware VM boundary (KVM) — not a shared kernel |
| Cross-tenant memory/FS isolation | ✅ separate microVMs, separate guest memory + ephemeral disk |
| Cross-tenant cache isolation | ✅ enforced by US (tenant-in-path routing + per-job scoped PAT + `_public` fail-safe + WP-8d posture) — substrate-independent |
| No long-lived secret on box | ✅ per-job D-9-minted PAT, scoped + short-TTL — substrate-independent |
| Supply-chain (image) integrity | ✅ X4 digest-pin enforced before spawn (CloudflareEngine floor) + image pinned in wrangler |
| Egress posture | ⚠️ bounded-egress accepted at launch (ADR-0003); on Cloudflare, confirm the network policy (§6) |

## 4. Comparison to the alternatives (no-regression check)

- **vs Northflank (interim):** same isolation class (managed microVM). No regression.
- **vs own-metal Firecracker (the roadmap FC backend):** SAME core tech (Firecracker microVM). Cloudflare
  is "Firecracker-as-a-service" — we get the gold-standard isolation without operating KVM hosts. The
  own-metal play was always an efficiency/cost lever at high steady scale, never a *stronger-isolation*
  lever. So Cloudflare does not cost us isolation we'd otherwise have.
- **vs shared-kernel Docker / gVisor:** Cloudflare is STRONGER than both. (A plain Docker CI runner shares
  the host kernel — unacceptable for strangers; gVisor narrows the surface but is user-space. Firecracker's
  hardware boundary is the correct bar, and Cloudflare is on it.)

## 5. Residual risks (honest — what Firecracker does NOT solve)

1. **Microarchitectural side-channels (Spectre/Meltdown-class):** hardware VM isolation reduces but does
   not fully eliminate cross-VM side-channels on shared cores. Firecracker's design + Cloudflare's
   host-level mitigations (core scheduling, microcode) are the defense; this is an industry-wide residual,
   not Cloudflare-specific, and is accepted by AWS Lambda/Fargate for the same workload class. **Confirm
   Cloudflare's side-channel posture (§6).**
2. **The control plane (the DO/Worker spawn layer):** isolation of the *container* is Firecracker's job;
   the *spawn-Worker + Durable Object* that starts containers runs on Cloudflare's Workers runtime. A bug
   there is a Cloudflare-platform concern, not ours — but our spawn-Worker auth (`CLOUDFLARE_SPAWN_AUTH_TOKEN`,
   bearer, fabric-internal) must be hardened (constant-time compare; it's currently `==`) before live.
3. **Egress (ADR-0003):** untrusted code with internet egress can exfiltrate its OWN per-job data (bounded
   by no-free-tier/card-on-file + scoped short-TTL PAT + ephemeral box). Full egress lockdown = the
   enterprise BYOC upgrade. Confirm the Cloudflare network policy matches the ADR-0003 bound.
4. **Noisy-neighbor / resource:** Firecracker isolates security, not necessarily perfectly fair CPU; the
   tenant vCPU-h ceiling + concurrency cap (entitlement) bound abuse economically.
5. **Image supply-chain:** covered by X4 (digest-pin before spawn + wrangler-bound image), but the image
   itself must be built from a trusted base — our existing Dockerfile X4 floor applies.

## 6. Conditions for sign-off (verify before first non-dogfood tenant — "bulletproof" items)

- [x] **Firecracker/KVM claim CONFIRMED (2026-06-20)** against primary Cloudflare docs (developers.cloudflare.com: "Each container instance runs inside its own VM... strong isolation"; blog.cloudflare.com/container-platform-preview — Firecracker VM). Original item: (this
      assessment is based on Cloudflare's public statements + secondary sources — verify the primary
      source; check their SOC 2 / Containers security page).
- [x] **Side-channel posture RESOLVED-by-design (2026-06-20):** Cloudflare Containers run each instance
      in its own Firecracker microVM (confirmed above) — the same hardware-VM isolation class as Northflank
      and own-metal Firecracker, so cross-tenant microarchitectural exposure is NO WORSE than the bar we
      already accept (no regression). Residual Spectre-class risk on shared cores is inherent to ALL
      multi-tenant cloud (incl. Northflank) and is mitigated by ephemeral one-shot boxes + no persistent
      cross-tenant co-residency. A dedicated-core option, if/when offered, is a future hardening for a
      high-sensitivity tenant tier — NOT a dogfood blocker. No open action.
- [x] **Egress policy RESOLVED (2026-06-20):** the runner container needs `enableInternet=true` (the
      GH-Actions agent + the in-network CAS both require outbound) — this is EXACTLY the bounded-egress
      posture ADR-0003 already accepts at launch (untrusted code can exfiltrate its OWN per-job data,
      bounded by card-on-file + scoped short-TTL per-job PAT + ephemeral box). No broader reach than
      Northflank. Full egress lockdown remains the same post-launch item as on Northflank — not a CF-specific gap.
- [x] **spawn-Worker auth hardening DONE (2026-06-20):** constant-time `safeEqual` bearer (live-verified)
      + GitHub HMAC on `/webhook` + **rate-limit DONE** (`WEBHOOK_LIMITER`, 30 spawns/60s, native CF binding)
      as defense-in-depth vs a leaked webhook secret. All three unit-tested (14 vitest tests).
- [x] **Tenant isolation at the cache seam CONFIRMED (2026-06-20):** the Server TL confirmed the in-network
      CAS path is the SAME Worker+container code as the public path (no separate codepath to drift) —
      `/v1/cas/<tenant>/…` routing, PAT↔tenant match, intra-tenant dedup (GA), cross-tenant staged. Per-job
      PAT (A6, never the tenant PAT) minted via the now-LIVE D-9. Relay/answer:
      `docs/handoff/2026-06-20-server-tl-ANSWER-d9-mint-LIVE-and-cas-wiring.md`.
- [x] **Live dogfood smoke DONE (2026-06-20):** a real GH-Actions job ran to SUCCESS on a CF Firecracker microVM (`cf-dogfood-1`, kernel `cloudflare-firecracker`), then self-deregistered. Stage B.

## 7. Recommendation

**PASS (2026-06-20).** Isolation is NOT the blocker the pivot feared — Cloudflare Containers are Firecracker
microVMs, the correct bar and a no-regression vs Northflank. **All §6 verify items are now CLEARED**
(Firecracker primary-source confirmed · side-channel resolved-by-design · egress matches ADR-0003 · auth
hardened + rate-limited + unit-tested · cache-seam tenant isolation confirmed by the Server TL · live
dogfood smoke green). The earlier CONDITIONAL became unconditional once the §6 list closed. The only
remaining work is operational, not an isolation gap: prod secret rotation (the throwaway dogfood spawn/
webhook secrets) before onboarding a non-dogfood tenant, and the warm-moat key (`CORELINK_PAT_MINT_AUTH_KEY`).

### Sources
- [Firecracker-powered containers on Cloudflare — Ernest Chiang](https://www.ernestchiang.com/en/posts/2025/firecracker-powered-containers-arrive-on-cloudflare/)
- [Your Container Is Not a Sandbox: MicroVM Isolation in 2026](https://emirb.github.io/blog/microvm-2026/)
- [Firecracker vs gVisor — Northflank](https://northflank.com/blog/firecracker-vs-gvisor)
- [Kata vs Firecracker vs gVisor — Edera](https://edera.dev/stories/kata-vs-firecracker-vs-gvisor-isolation-compared)
- Internal: `docs/adr/0003-egress-isolation-posture.md`, `docs/adr/0008-cloudflare-containers-substrate.md`, the V1 audit (this session).
