# ADR-0003 — Network-isolation posture for untrusted compute on the managed sandbox tier

- **Status:** Accepted (owner decision, 2026-06-12)
- **Date:** 2026-06-12
- **Context source:** validated live against the Northflank API (team `humangr`,
  project `corelink-runners`, region `us-east1`) during the CloudSandboxEngine
  bring-up. Supersedes the "SECURITY — DEPLOY GATE" placeholder in
  `crates/corelink-cloud-engine/src/northflank.rs` (which flagged egress as an
  unresolved blocker before this decision).

One line: on the managed sandbox tier, **cross-tenant network isolation is the
hard guarantee** and **outbound internet egress is accepted at launch** — the
accepted risk is bounded by the no-anonymous-user product model, not by an
egress firewall (which is a BYOC-tier upgrade).

## Context

Runners execute customer (and AI-agent) code. The execution substrate at launch
is a managed microVM sandbox provider (Northflank — see
`[[corelink-runners-execution-stack]]` and `docs/product/pricing.md`), not own
metal. Live investigation established two facts:

1. **Cross-tenant isolation exists by default.** Northflank deploys Cilium
   network policies between projects (namespaces); with multi-project networking
   **off** (our project config), a job cannot reach another tenant's project.
   This is the tenant-isolation property that matters for multi-tenant safety.
2. **Per-job outbound-internet blocking is NOT a managed-PaaS primitive.** Full
   egress lockdown (egress gateway / static egress IP / deny-all-outbound) is a
   **Bring-Your-Own-Cloud** feature — it requires operating your own cluster.
   The managed create-job spec exposes no per-job no-egress toggle (confirmed:
   a created job has `ports: null` — no ingress — but no egress directive).

So full egress lockdown on the managed tier would mean going BYOC, which
contradicts the deliberate "managed, no metal at launch" strategy.

## Decision (owner)

**Accept outbound internet egress at launch (Option A).** Do not gate launch on
egress lockdown and do not adopt BYOC for it now.

The accepted risk is bounded — structurally, not by hope — by the product model
and the existing isolation discipline:

- **No anonymous untrusted code.** There is **no free tier**; every account is
  card-on-file with a 5-day trial (`docs/product/pricing.md`). The classic
  egress nightmare is *anonymous* code with internet (exfiltration, C2, abuse
  source). We have no anonymity: every job traces to an identified, billable
  account, so abuse is attributable and costly to the abuser.
- **Cross-tenant isolation is hard** (Cilium, multi-project off) — a job cannot
  reach other tenants.
- **Secrets are brokered, never on the box** (inherited C5b discipline) — egress
  cannot exfiltrate platform secrets that were never present.
- **Ephemeral microVM-per-job**, torn down after.
- **Cache-warm reduces real egress need** — deps come from the CAS/AC, not the
  open internet, so legitimate workloads rarely need egress at all.
- **Anti-abuse** — sustained-pin / mining detection on `CapGate` (pricing.md §5).

For most CI/build workloads, *some* egress is expected and desired anyway (npm,
apt, git) — the same posture every mainstream CI (GitHub Actions, Depot,
Buildkite) ships. We are not weaker than the category default; we are stronger on
cross-tenant isolation and on identity.

## Consequences

- **Launch is unblocked** on the managed tier; no BYOC, no own metal.
- The `northflank.rs` module security note is reframed from "blocker" to this
  decided posture and references this ADR.
- **BYOC egress lockdown is the named enterprise upgrade.** When an enterprise
  customer contractually requires deny-all / allowlisted egress, the answer is a
  BYOC deployment with an egress gateway — sold as the Enterprise/governance tier
  (consistent with the CoreLink governance differentiator in pricing.md), not a
  managed-tier default.
- **Revisit triggers:** (a) introducing any anonymous/free execution path
  (would remove the identity bound — re-open this decision); (b) a concrete
  exfiltration/abuse incident; (c) a customer egress-lockdown requirement
  (route to BYOC). Any of these re-opens the posture.
- This is a posture decision, not a contract change — the frozen CoreLink
  integration contract and the wire types are untouched. Historical external
  integration framing does not create a current consumer or launch gate.
