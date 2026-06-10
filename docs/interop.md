# CoreLink Runners interop — how this repo talks to the family (microscopic seam map)

> 2026-06-09. Runners is a **layer on the cache, consumed from above**: hugit
> (anchor tenant) and Workspaces (sandbox/dev-box SKUs) ride it; it rides
> CoreLink's CAS/AC/tenancy. Status: **spec phase (M0)** — no fabric code; the
> consumer side (hugit's `hugit-runner` client) is built and runs today against
> the interim box `hugit-runner-01` (Hetzner, SSH). Canonical seam:
> `docs/spec/hugit-integration-contract.md` (frozen from hugit's side — the
> fabric satisfies it, never edits it).

```
   hugit (built) ──lease/exec/attest──▶  RUNNERS FABRIC (this repo, to build)
   Workspaces (M4: sandboxes/dev boxes) ──▶      │ consumes, never forks
   direct customers (M2: concurrency plans) ──▶  ▼
                                     CoreLink Cache — CAS·AC·R2·tenancy·PAT (live)
```

## 1. Serving hugit (the anchor tenant) — the exact loop

Per check, the fabric's obligations (full context in the frozen contract):

| Step | Microscopic detail |
|---|---|
| 0. Memo first | hugit computes `H(tree ‖ check_def ‖ toolchain)` and asks the **AC**; hit ⇒ no lease is ever requested. The fabric only sees **misses** |
| 1. Lease | request `{tenant, size, image (sha256-pinned), ttl, claim (FenceManifest)}` → `RunnerLease {lease_id, exec endpoint, deadline}`; states `Pending → Held → (Released | Expired | Crashed)` — no invented intermediate states |
| 2. Warm boot | CAS/AC working set local **before the first instruction** (toolchain, deps, source-by-content). Cache unreachable ⇒ **explicit error** — never a silent cold result dressed as warm |
| 3. Execute | `CheckDef` → `CheckResult` (exit, canonical bytes, duration, output content-digest). **Byte-identity is sacred**: same def + inputs ⇒ same digest on any runner, any time; the fabric controls the knobs (clock, RNG, locale, paths, parallelism, artifact timestamps) and injects no per-boot values |
| 4. Isolate | fresh fail-closed microVM per lease; `FenceManifest` enforced (covers `..` escape, absolute-path injection, `srcfoo` vs `src/` prefix collision); strict tenant isolation (HMAC-prefix boundary); box destroyed after |
| 5. Secrets | brokered: never on image/disk/argv; credential-scan attestation `env=0, proc=0, disk=0`, fail-closed if unparseable |
| 6. Attest + store | signed `{image digest, resolved inputs, result hash}` → hugit's `AttestationChain`/transparency log; result stored under the memo key. **No/invalid attestation ⇒ hugit rejects the result** |
| 7. Expiry/crash | `ttl` kill ⇒ `Expired`, **no partial result stored, ever**; crash ⇒ `Crashed`, hugit retries on a new lease, no duplicates |
| Trigger path | `QueueApi`: hugit's landing queue calls the fabric on uncached checks (red PR, new tree); auto-bisect follows on failure |
| Honest accounting | exec vs cache-hit reported truthfully (no inflated "served from cache") — githugr renders these numbers as product KPIs |
| Caps & fairness | per-tenant rate + concurrency/budget caps set **before** load (X10⑤ preventive); p95-wait fairness under contention (C7); other-tenant latency unmoved under hugit storms (X6/X10 — measurable) |

**Definition of done for the seam:** hugit's gated suites flip live (run-not-skip
with the endpoint set): B2b byte-identity · C3 warm<cold + cache-down
fail-closed · C2a/C2b/C9 lease lifecycle · C5b secrets red-team · X11 mid-op
broker fault · X6/X10 non-interference.

## 2. Consuming CoreLink Cache

CAS/AC/R2 + tenancy + PAT, as a layer — never reimplemented. Auth: Bearer PAT
(same scheme as the cache product). Warm-boot reads and result-stores go
through the same content-addressed store every other product shares. Dedup is
intra-tenant today; cross-tenant of public-deterministic artifacts is staged
post-GA (`CAP-DEDUP-CROSS-TENANT`) — see
`docs/review/2026-06-09-cross-tenant-dedup-claim.md` for the tense discipline.

## 3. Serving Workspaces (M4 adjacency)

Agent sandboxes and cloud dev boxes are **Workspace SKUs that run on this
fabric**: same lease/isolation/attestation spine, workspace manifests as the
materialized state (`clw snapshot/hydrate` family). Nothing is duplicated:
Workspaces sells the object, Runners executes beside it.

## 4. The two front doors (one fabric)

- **Via hugit:** invisible execution substrate beneath memoized CI — a hugit
  customer never sees a "Runners" line item (COGS under hugit's flat plan).
- **Direct (M2):** self-serve concurrency plans (flat per parallel runner,
  unlimited minutes), GitHub-Actions-shim on-ramp. Onboarding via the **HuGR
  account** (ADR-0002: `docs/adr/0002-hugr-identity.md`) — same Clerk pool,
  org = tenant keys caps/fairness/billing.

## 5. The interim → M1 handover (stated honestly)

Today hugit's CI executes on `hugit-runner-01` (Hetzner) over SSH with
`docker run` (pinned known-hosts; env `HUGIT_RUNNER_HOST`) — dogfood-grade
transport that lights live CI at **P2**. **M1 replaces the transport, not the
contract**: same `RunnerLease` semantics behind an authenticated API, plus
multi-tenancy, caps, and attested execution at production grade. The contract
was written so hugit's client does not change.

## 6. Change protocol

The hugit contract is frozen from hugit's side — a fabric-side need to change
any shape or guarantee escalates to the owner / hugit techlead; never assume
hugit adapts. Family decisions are ADRs, canonical in
`../hugit/docs/adr/` (this repo carries thin companions).
