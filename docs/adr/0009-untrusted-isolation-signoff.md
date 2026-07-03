# ADR-0009 — Untrusted multi-tenant isolation: sign off on Cloudflare Containers (Firecracker-class), keep own-metal Firecracker as the frozen upgrade path

- **Status:** Accepted
- **Date:** 2026-07-02
- **Decided by:** corelink-runners tech lead (owner-delegated the sign-off 2026-07-02; owner is stakeholder and may override)
- **Supersedes the open item in:** ADR-0008 §"Gated" (isolation security review), `deploy/cloudflare/README.md` isolation sign-off gate

## Context

The product runs **untrusted code** (customer CI + AI-agent code) on shared infrastructure. The load-bearing security question, left open by ADR-0008, is: **does Cloudflare Containers' isolation meet the "Firecracker-class microVM" bar for untrusted multi-tenant compute, or must we operate our own Firecracker on bare metal?**

The isolation-strength ladder (weakest → strongest): shared-kernel container (one kernel bug = escape) → gVisor user-space kernel (smaller surface) → **microVM/Firecracker** (own kernel per VM; escape needs a hypervisor breakout, the AWS-Lambda/Fargate gold standard) → separate metal.

## What we confirmed (the load-bearing fact)

From Cloudflare's own documentation (2026-07): **each Cloudflare Container instance runs inside its own VM.** Cloudflare runs **Firecracker** (VM boundary) and **gVisor** (user-space kernel) and deliberately does not force the choice; a build runs in a **Firecracker VM, thrown away per job**, next build gets a fresh prewarmed one. Cloudflare **designs and markets this explicitly for untrusted code execution** per-user/per-session (the Sandboxes product rides the same substrate). See Sources.

Our own spawn path enforces the matching invariants: **one container Durable Object per job** (`deploy/cloudflare/src/index.ts:77`), **idempotent `destroy()` teardown** (`index.ts:100-102`), pinned **`standard-4`** VM size (`wrangler.jsonc:60`), one-lease-one-box never reused (CLAUDE.md invariant). So: **a fresh microVM per lease, destroyed after — no VM reuse across tenants.**

## Decision

**Sign off. Cloudflare Containers meet the Firecracker-class bar. Ship untrusted multi-tenant CI on Cloudflare Containers.** Own-metal Firecracker (ROADMAP FC1–FC5) stays the **documented, frozen upgrade path behind the `isolation::Engine` seam** — invoked only if we ever must leave CF, not a launch blocker.

### Why

1. **The boundary already IS a microVM.** It is literally Firecracker under the hood — one VM per job, discarded after. Escape requires a hypervisor breakout, not a shared-kernel bug. That is the bar, met — not a weaker substitute.
2. **Cloudflare owns the hypervisor hardening, for exactly this threat model.** They build and operate this substrate *for untrusted code*. Their security team hardens the VMM surface far better than we could hand-rolling KVM on bought metal.
3. **Track-C stacks on top as defense-in-depth on the software boundary** — env-0 creds (PAT never in the untrusted env), lease-bound PAT TTL + revoke-on-teardown, cap-drop / no-new-privileges / pids+memory limits, per-tenant repo allowlist, tenant-suspend enforcement. Result: **CF microVM boundary + our hardened software boundary = layered**, not single-point.
4. **Own-metal Firecracker is strictly worse right now:** the *same* technology (Firecracker), but we operate it worse, we **lose R2 co-location** (zero-egress cache — the moat), and we must buy + run KVM hardware (ratified decision #5 already blocks it on that buy). No security upside over CF's Firecracker; real operational + moat downside.

### Conditions attached to the sign-off (not a blank check)

1. **One-tenant-per-VM, fresh per lease, destroyed after.** VERIFIED in the spawn path (one DO per job + `destroy()` teardown). Any change that reused a VM across leases/tenants voids this ADR.
2. **The VM (Firecracker) boundary must hold** — keep the `standard-4` VM instance type; do not move untrusted CI to any lighter shared/co-tenant container mode without re-review.
3. **Track-C stays ON in production** — the software hardening is mandatory even under a strong sandbox (defense-in-depth), never treated as redundant.
4. **Re-review trigger:** if we ever co-tenant multiple tenants *inside a single VM* (we do not, and should not). One-lease-one-box keeps cross-tenant escape gated behind a hypervisor break.

## Residual risk (stated honestly)

- **Shared-fate with Cloudflare's hypervisor security.** We depend on CF not having a Firecracker/VMM escape. This is the same bet every AWS-Lambda/Fargate customer makes on a top-tier provider — accepted, and strictly better than us operating a VMM ourselves.
- **Not covered by this ADR:** the R2 co-location credential seam (in-network CAS creds) is a separate **Cache-TL** item; the 12 GiB RAM ceiling validation against the heaviest builds; and the live-deploy ops gates in `deploy/cloudflare/README.md`. Those remain open but are operational, not isolation-bar, questions.

## Consequences

- The `deploy/cloudflare/README.md` "Isolation security review — owner sign-off" gate is **satisfied by this ADR** (subject to its 4 conditions).
- ADR-0008's open isolation item is **closed**.
- FC1–FC5 (own-metal Firecracker) remain **deferred, off critical path**; the Engine v2 seam stays frozen so the swap is a backend addition if ever needed.

## Sources

- [Cloudflare Containers — Global Container Platform](https://www.cloudflare.com/products/containers/)
- [Containers are available in public beta](https://blog.cloudflare.com/containers-are-available-in-public-beta-for-simple-global-and-programmable/)
- [Our container platform is in production. It has GPUs.](https://blog.cloudflare.com/container-platform-preview/)
- [Cloudflare Sandboxes — Secure Code Execution](https://www.cloudflare.com/products/sandboxes/)
- [Lifecycle of a Container · Cloudflare Containers docs](https://developers.cloudflare.com/containers/architecture/)
