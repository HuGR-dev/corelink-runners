# CoreLink Runners — roadmap

> Owner: HuGR TechLead · baseline: audit 2026-06-11 on `integ/seed-runner`
> (post runner-seed, full gate verified green locally: fmt · clippy `-D warnings` ·
> 141 tests · deny · audit · conformance hashes byte-checked).
> Evidence: `docs/handoff/2026-06-10-runner-seed.md` ·
> `docs/spec/hugit-integration-contract.md` v1.2.0 ·
> `docs/whitepaper/corelink-runners-v1.md` (M1 bar).

Seeded ≠ shipped. What is green today is the **execution core** (single box,
Docker isolation, SSH transport). The **product** is M1. This file tracks the
distance between the two; one line per item, struck through when closed.

## P0 — seed hardening (current wave, in flight)

The contract obligations and audit findings that are closable **inside this
repo, now**, with no owner input:

- [x] **§13.1 metrics envelope** — transcribed `IntentMetrics` type +
      golden fixture + derivation collector (`558842e`, `fdccc33`).
- [x] **§13.2 capture hook points** — CaptureHook (two surfaces, bearer
      seam, progressive forwarding) + JobClose ack state machine (`31157e6`).
- [x] **§13.3 no-persistence** — bounded in-flight only, release-after-
      delivery, overflow never silent; proven by test (`31157e6`).
- [x] **Conformance manifest hash-verify** — real SHA-256 per vector +
      membership pin + tamper-mutation proof (`558842e`).
- [x] Fixup squashed; contract title + CLAUDE.md at v1.2.0; transplant prose
      fixed (`9c86744`). deny.toml `Zlib` kept deliberately (house set ≡ hugit).

Scope note (recorded, not silent): §13 lands as the **mechanism** (module with
the contracted semantics + acceptance suite). Wiring it into the production
lease/API path is M1 work — the obligation binds "when the runner product
hosts agent-driven execution", i.e. the M1 fabric.

## P1 — ship the seed (owner-gated: every item needs Gustavo)

The repo is local-only today: no remote, no backup, CI has never executed as
CI, and branch→PR→merge is physically impossible. To ship the seed milestone:

- [ ] Create the GitHub repo + remote (org/name = owner call).
- [ ] Decide default branch: CLAUDE.md says PR to `main`; only `master`
      exists; `ci.yml` triggers on `master`. Align all three.
- [ ] Register the self-hosted runner (`corelink-builder`) with the repo.
- [ ] Push → PR `integ/seed-runner` → default branch → first real CI run
      green → merge → tag `v0.1.0-seed`.
- [ ] **Cross-repo `IntentMetrics` conformance vector** — §13.4 requires it
      byte-identical in both repos; `../hugit/conformance/` does not have it
      yet either. Needs a hugit-side PR (hugit techlead) + mirror here.
- [ ] **`hugit-c9-` container-prefix rename decision** — ops-visible on the
      shared interim box; rename is a seam change, not a local cleanup.

## M1 — the production fabric (campaign; decompose when P1 closes)

The bar (whitepaper): multi-tenant behind the same `RunnerLease` semantics —
caps enforced before load, p95 fairness, measurable non-interference,
byte-determinism, signed attestation. "M1 replaces the transport, not the
contract." Epics:

- [ ] Multi-tenant control plane — scheduler, per-tenant caps, fairness.
- [ ] Public lease API — REST acquire/status/cancel, PAT auth (CAS/AC pattern).
- [ ] Billing — per-concurrency-slot metering, org→tenant keys, invoicing;
      **concurrency pricing, never per-minute** (decided principle).
- [ ] Firecracker isolation engine — microVM per job behind the existing
      engine-agnostic `ContainerSpec`/`BoxExec` seam; same teardown oracle.
- [ ] §13 wiring — envelope hooks + metrics on the production job-close path;
      authenticated (Bearer PAT) hook subscription.
- [ ] Attestation chain at the API surface (§7) + secrets-broker wire seam
      (§5; C5b implementation stays forge-side).

## M2 — direct GA

- [ ] Identity via the HuGR account (ADR-0002: same Clerk pool; org = tenant).
- [ ] Self-serve onboarding for the direct ICP (infra/CI teams); same fabric,
      second front door — hugit never sees a "Runners" line item.

## Standing constraints (do not relitigate without the owner)

Concurrency pricing · never bill the customer's compute twice · cache-warm by
construction · fail-closed isolation · tense discipline on cache claims ·
integration contract frozen from hugit's side (§12 protocol).
