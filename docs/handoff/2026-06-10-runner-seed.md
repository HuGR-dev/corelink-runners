# Runner seed — handoff to campaign-#1 sessions

**Date:** 2026-06-10  
**From:** hugit TechLead (runner-transfer campaign, WP-R5)  
**To:** CoreLink Runners campaign-#1 sessions  
**Branch:** `integ/seed-runner` (PR to `main` pending full campaign closure)

---

## What arrived

The proven ephemeral-runner v0 has been transplanted from hugit into this
workspace. Every module arrived intact with zero behavioral change; the transfer
was lead-verified gate-green at each step.

### Execution core — `crates/corelink-runner` @ b6319a3

Full module inventory (provenance: hugit `crates/hugit-runner` @ ead800d):

- **`lease`** — RunnerLease lifecycle (acquire · hold · release · expiry)
- **`isolation`** — container isolation model (per-lease sandbox, fail-closed)
- **`teardown`** — forensic teardown (artifact collection before destroy)
- **`pin`** — supply-chain image pinning (`@sha256:` enforcement)
- **`boot/`** — warm boot from CAS-pre-warmed image
- **`concurrency/`** — multi-job concurrency + capacity management
- **`expiry/`** — lease TTL enforcement
- **`recovery/`** — crash / partial-failure recovery paths
- **`shim/`** — Actions-YAML compatibility (broker, executor, parser, report, subset)
- **`ws/`** — dedup spawner (one container per distinct claim under the lease)
- **`materialize/`** — sparse hydration: path-set-driven CAS materialization (C5a)
- **`enforce/`** — in/out classifier + box-backed ENOENT probe (C5a)
- **`redteam/`** — six container-escape vectors; the fence-materialized-escape
  vector is load-bearing (goes RED under a no-op classifier) + hermetic FakeFsBox
  twin for the bare gate

### X4 supply-chain oracle — `crates/corelink-runner/x4` @ b6319a3

Proves the live spawn surface: content-pinning + verify-before-spawn fail-closed
ordering over `ContainerSpec::from_lease()` + `Engine::spawn()`. The hermetic
ordering proof runs in the bare gate; item ② retargeted to THIS workspace's
pinned lockfile/CI (same invariant, honest home). hugit retains only a
wire-level conformance assertion in `hugit-invariants` — rigor preserved by
relocation, not weakened.

### Wire-contract types — `crates/corelink-runners-contracts` @ 78702d6

Transcribed from hugit-contracts @ 7c2f1e6 (frozen, unrelocated):

- `RunnerLease`, `RunnerState`, `FenceManifest` — the runner's whole
  hugit-contracts surface (nothing more — R0 freeze)
- `MaterializedEntry` — closure type required by `materialize/`
- All types: `deny_unknown_fields`, `schema_version`, `serde` attributes matching
  the hugit originals, provenance note per type

### Conformance vectors — `conformance/` @ 78702d6

`RunnerLease.json` + `FenceManifest.json` + `manifest.sha256` committed
byte-identical to hugit. The golden round-trip tests in `corelink-runners-contracts`
pin every vector byte-exact. These vectors are the **drift tripwire**: if a type
drifts from hugit's frozen originals, the golden tests break — the divergence is
never silent.

Verified digests (lead-confirmed):
- Manifest SHA-256: `159fe8c5…`
- `RunnerLease` vector: `ab1744c9…`
- `FenceManifest` vector: `07940b9a…`

### Integration contract — `docs/spec/hugit-integration-contract.md` v1.1 @ 9796aa8

v1.0 → v1.1: added the runner's obligation to emit per-job metrics consistent
with `IntentMetrics` and the capture hook points for trajectory blobs (full +
compacted — the two-transcript imperative), version-bumped with amendment log,
cross-referencing hugit ADR-0001. This closes the envelope-emission dependency
flagged at WP-R6.

### Acceptance suites — all green

| Suite | Verdict |
|---|---|
| `tests/acceptance_c2a.rs` — lease lifecycle | green |
| `tests/acceptance_c2b.rs` — isolation/teardown | green |
| `tests/acceptance_c3.rs` — boot/warm/concurrency | green |
| `tests/acceptance_c9.rs` — harness lifecycle (attach/resume/dedup/local≡remote) | green (box-lane: FAIL-not-skip when `HUGIT_RUNNER_HOST` set) |
| `tests/acceptance_e4.rs` — end-to-end harness | green |
| `tests/acceptance_redteam.rs` — container-escape red-team (C5b item ⑤) | green |
| `tests/acceptance_x4.rs` — X4 supply-chain oracle | green |
| `tests/hermetic_supply_chain.rs` | green |

Full gate: `fmt` · `clippy -D warnings --locked` · `test --locked` ·
`cargo deny check` · `cargo audit --deny warnings`.

---

## What it proves

The transplant proves the execution core is sound and self-contained:

1. **The lease-container-teardown loop is proven end-to-end** (C2a/C2b/C3/E4) in
   this workspace, against this workspace's types — not a copy of hugit's proofs
   but the same tests running against the same code in their new home.
2. **Fence enforcement is real** — `materialize` + `enforce` drive the LIVE
   classifier in-process; the red-team vector that would go RED under a no-op
   classifier stays RED in the bare gate (relocated, never weakened).
3. **Supply-chain pinning is proven over the live spawn surface** (X4) — the
   verify-before-spawn ordering holds in-process; no behavioral bypass is
   reachable.
4. **The wire seam is frozen and tripwired** — types are transcribed on both sides,
   conformance vectors pin them byte-exact; a future type drift cannot be silent.
5. **Envelope emission is contracted** — the v1.1 contract specifies the per-job
   metric obligation and the capture hook points for trajectory blobs; sessions
   building the control plane must satisfy it.

---

## What the PRODUCT still needs

**Seeded ≠ shipped.** Everything above is the execution core. CoreLink Runners
the product requires:

| Gap | Notes |
|---|---|
| **Multi-tenant control plane** | Scheduler: assign leases across tenants; enforce per-tenant concurrency caps (a tenant buys N slots); fairness between tenants. None of this is in the seed. |
| **Public API** | REST surface for lease acquisition, status, cancellation. The seed has in-process interfaces only. |
| **Billing** | Per-concurrency-slot metering, Clerk-org → tenant key, invoice emission. The seed has no billing primitives. |
| **Firecracker isolation** | The seed runs Docker/container isolation (suitable for a trustworthy fleet like the interim box). Production hostile-tenant isolation needs Firecracker microVMs or equivalent. |
| **Secrets broker (C5b)** | The broker (`hugit-fence::broker`) stays hugit-side with the forge (it is a forge-domain concern: PATs, claim-fenced delivery). This repo's fence surface (`materialize`/`enforce`) is the runner-side half; it consumes from the broker via the wire contract. |
| **Live box provisioning** | `hugit-runner-01` day-0 infra pointers (IP, SSH, GitHub App ID, secret paths) are in hugit's `docs/handoff/`. The box is not this repo's asset to manage. |
| **M1 fabric** | The production multi-tenant fabric behind the same `RunnerLease` semantics. The seed is the interim-box implementation (SSH + Docker). M1 is the campaign's destination. |

---

## Obligations added by v1.1 (read before building the control plane)

The integration contract v1.1 adds two runner-side obligations:

1. **Per-job metric emission:** after each job completes, emit a metrics payload
   consistent with `IntentMetrics` (tokens/cost/duration decomposed by
   work/orchestration/verification/ci/waste) to the capture hook point.
2. **Trajectory blob capture hook points:** expose two hooks — one for the full
   transcript and one for the compacted trajectory — so hugit's envelope producer
   (`hugit-ledger::envelope`) can pull them. The two-transcript imperative (full +
   compacted) is non-negotiable; both hooks must be wired before the runner
   processes production intents.

Cross-reference: hugit ADR-0001 (context envelope, owner-ratified 2026-06-10).

---

## Multi-session warning

`integ/seed-runner` may have other live sessions. Before any write:
1. `git status` — must be clean.
2. `git log -1` — confirm HEAD is the commit you expect.
3. Never `git commit --amend`. Fixup commits only.
4. The session fence (`.claude/settings.json` + `.claude/hooks/forbid-sibling-paths.py`)
   enforces repo isolation; open sessions IN this directory.
