# CoreLink Runners — E2E Validation Suite (the "real user in prod" spec)

**Date:** 2026-07-19 · **Lead:** runners TL (techlead) · **Trigger:** owner — *"uma suite e2e que
simula um usuário real em prod, mesmas limitações e poderes que um usuário tem; todas as features,
todos os cenários, todas as direções possíveis"* + two guarantees demanded before any run:
**(G1)** the suite is provably complete, and **(G2)** every cell validates real *behavior* + captures
*total evidence* (logs, security, side-effects) — not just an HTTP `200`.

This is the SPEC + COVERAGE MATRIX. It is the artifact the owner reviews to grant the run-go. It does
**not** run anything; it defines *what* the suite must cover, *how* each cell proves behavior, and the
honest line between **fabricable** (I produce evidence) and **X4-blocked** (needs an external input).

---

## 0. The honest framing (rigor compact — read first)

"Literalmente todo cenário possível" is combinatorially **infinite**; no honest engineer promises it,
and the rigor compact forbids me from claiming it. What this suite guarantees instead is **stronger
because it is auditable**:

> **Completeness-by-construction.** Every cell of the suite is *derived mechanically* from the product
> ledger — **55 features** (`docs/product/FEATURES.md`, F-1.1…F-10.5) × **155 user stories**
> (`docs/product/USE-SCENARIOS.md`, 17 personas) — crossed with **4 directions** (happy · edge ·
> adversarial · failure) and the **load-bearing combinations**. A machine **completeness-critic**
> (§4) proves that **no F-id and no S-id is left unmapped**: an orphan atom is a *build error*, not a
> silent gap. Every cell that cannot be run to its target grade becomes an **explicit, cited GAP** with
> the reason and the exact input needed — never a silent "validated".

That is the only truthful reading of "todos os cenários". It is auditable, re-runnable, and it fails
loud when coverage regresses.

**Framing caveat (honesty).** The "210 atoms" are **two axes**, not 210 disjoint things: 55 *capabilities*
(features) × 155 *journeys* (scenarios). A journey exercises several capabilities; the count is the size
of the coverage grid, not a list of unrelated items. The critic enforces coverage on *both* axes.

### Review verification (2026-07-19, evidence captured — not theory)
Probed against the live fabric (`corelink-fabricd.gmhelmold.workers.dev`) at review time:
- **Substrate up:** `GET /health` → `200`; `GET /v1/attestation/key` → `200` (FLIP-B key serving).
- **Auth fail-closed:** `POST /v1/leases` with no PAT → `401`.
- **Real authenticated-user power CONFIRMED:** `POST /v1/leases` with the standing f0005 tenant PAT +
  a deliberately-invalid image → `400` (image validation runs *after* auth) — proves the PAT
  authenticates live. (Caveat: a *valid* acquire spawns a real box — real cost/side-effect; and the moat
  test-mint endpoint is currently **disarmed**, so moat-mint cells either re-arm `FABRIC_TEST_MINT_KEY`
  or ride the Door-A dogfood path.)
- **MULTI-TENANT UNLOCK (D2 is no longer X4), 2026-07-19:** the CoreLink e2e env (`~/.corelink/secrets/
  e2e-prod-env.sh`) carries **multiple real tenant/tier PATs** (Free/Solo/Pro/Enterprise + a **second
  tenant B**), all introspect-valid → all authenticate against fabricd. Proven live (no spawn):
  **entitlement scales per-tenant** (`plan_cap` Free=1 · TenantB=2 · Pro=10 · Enterprise=100 — the
  ratified 10× ladder), and **cross-tenant scoping holds** (each PAT resolves to its own distinct
  tenant id; no PAT reads another's usage). So **D2 multi-tenant + entitlement + the usage-API reads
  are fabricable E2**, not X4. Residual X4: multi-tenant **billing attribution under concurrent load**
  (needs a spawn-bearing run) and the **external-org install click**.

### Disposition — the honest counts (deterministic, by ledger badge)
Not an estimate — counted from the H3 badges (55 F + 155 S):
- **Fabricable = 163** (🟢 97 LIVE-proven, re-verified · 🟡 66 built-not-proven, *the suite's core job to raise*).
- **Owner/external-gated = 43** (🔵). Contains BOTH pure owner-flips (GA billing, vCPU-wall, RAISE-N)
  AND the external-actor-gated cells (external-org install click, 2nd real tenant). These are the
  load-bearing X4 dependencies even though the ledger badges them 🔵.
- **X4-pure = 2** (⚪ — S2.1.1, S2.1.3, direct CoreLink check/SDK cells awaiting their live fixture; no external project is required).
- **Planned / campaign #2 = 2** (⚫ — F-2.4 Workspaces door, F-5.10 multi-size resolver).

A few 🟡 carry an X4 *tail* (e.g. full `[clw] cache hit`, S2.5.2 two-front-doors) — reflected per-atom
in the appendices, not hidden in the headline.

---

## 1. The two guarantees, mechanized

### G1 — provable completeness (the completeness-critic)
- **Input of record:** the frozen atom inventory in Appendix A (55 F-ids) + Appendix B (155 S-ids),
  extracted verbatim from the ledger.
- **The critic** (`suite/completeness.test.ts` + a Rust twin): asserts that **every** F-id and **every**
  S-id appears in `>= 1` cell of the coverage map, with a declared direction, target grade, and
  disposition (fabricable | X4). Any atom with zero cells ⇒ **the critic fails the build.**
- **Re-run law:** when the ledger gains an F-id/S-id, the critic goes red until a cell exists. Coverage
  cannot silently rot.

### G2 — behavioral evidence, not status codes (the evidence model)
Every cell is a **triple**: `(stimulus → behavioral assertion → captured artifact)`.

- **Stimulus** — the exact request/event/failure injected (curl, webhook, kill, fuzz).
- **Behavioral assertion** — what "correct" *means* beyond the code. One or more of:
  - **state transition** (lease `pending→held→closed`; PAT `minted→redeemed→revoked`),
  - **counter delta** (`spawn_at_ceiling` +N, `spawn_failed` +0, exact seam, no over/under-count),
  - **log line** (the loud log on the critical path actually fired),
  - **side-effect presence/absence** (box torn down; secret **absent** from env dump / logs / body),
  - **security invariant** (fail-closed 401/404; forced-server-side `net_policy`; single-use ticket).
  - A `200` whose behavior is wrong = **FAIL**. A `200` is necessary, never sufficient.
- **Captured artifact** — the raw evidence stored in the evidence ledger keyed by atom-id: response
  body, counter snapshot `t0→t1`, log grep, box-inspect, exit code. No cell is "green" without one.

---

## 2. The suite architecture (executable + re-runnable)

The suite is **one entrypoint** (`scripts/e2e/run.sh <suite|all>`) over six test-suites. Each cell
declares its `atoms: [F-…, S-…]`, `direction`, `grade`, `disposition`, and emits an evidence record to
`docs/validation/evidence/<run-id>/`. Grades: `E0 unit · E1 route/integration · E2 live-probe ·
E3 live-smoke · E4 live-stress · E5 chaos · X4 external`.

| Suite | What it drives | Powers/limits modeled | Grade | Where |
|---|---|---|---|---|
| **TS-1 Correctness** | every feature's happy/edge/failure path, behavior-asserted | the fabric's own logic | E0/E1 | `crates/**/tests`, `deploy/cloudflare/test/*` (extend) |
| **TS-2 Live-journey** | **the real user against live prod**: Door-A dogfood job, Door-B `acquire`, moat mint→redeem→cache, `corelink` CLI, `GET /v1/*` reads | exactly a real user's HTTP surface + a real dogfood App install | E2/E3 | `scripts/e2e/journey/*` (new) |
| **TS-3 Stress** | burst to fleet-cap **+ a bit over** → atomic slot + fleet cap + zero thrash | a real tenant bursting to N | E4 | `scripts/e2e/stress/*` (new) |
| **TS-4 Chaos** | kill fabricd mid-flight · inject spawn-fail · deps down · force a real canary breach · rollback | real failures + recovery | E5 | `scripts/e2e/chaos/*` (new, **owner-gated run**) |
| **TS-5 Security** | auth-fuzz **every** gate · secret non-leak · HMAC replay/timing · fence escape · supply-chain · cross-tenant · net_policy · single-use ticket | an **adversary** with a user's reach | E1/E2 + live | `scripts/e2e/security/*` (new) + red-team suite |
| **TS-6 External** | non-HuGR-Labs repo install + dispatch · 2nd tenant isolation/billing · real `[clw] cache hit` | a **real external customer** | X4/D | `scripts/e2e/external/*` (spec + gated) |

**Real-user fidelity (the "mesmos poderes e limitações" law).** TS-2/3/5 drive **only** the surfaces a
real user can reach — the public `/v1` API with a real PAT, the GitHub App path, the `corelink` CLI —
using the **live dogfood install** (App installation `150584374`, first-party). The privileged
`/internal` surface is used **only** to *read evidence* (counters), **never** as a stimulus a user
couldn't produce. Where a claim needs a real *external* actor (a 2nd org, a 2nd tenant, a real CoreLink
PAT for full cache-hit), it is **TS-6 / X4** — proven where fabricable, honestly gated where not (the
Lens-E law: never generalize a dogfood smoke to an external customer).

---

## 3. Coverage matrix — domain × direction (target grades)

Each feature/scenario atom is tested in the directions its nature admits. `✓` = a cell exists at the
listed grade; `X4` = highest honest grade needs an external input.

| Domain (atoms) | happy | edge | adversarial | failure | Highest live grade fabricable here |
|---|---|---|---|---|---|
| **Admission / caps / fairness** (F-5.2, F-1.3/1.4, S1.3.x, S5.2.3) | ✓E4 | ✓E4 | ✓E2 | ✓E4 | **E4** (burst to cap+bit) |
| **Lease lifecycle / reaper** (F-4.1, F-5.5, S1.2.4, S1.6.14) | ✓E3 | ✓E1 | ✓E1 | ✓E5 | **E5** (reaper under kill) |
| **Moat: mint / redeem / warm boot** (F-5.9, F-4.3, F-6.1, S1.2.1) | ✓E2 | ✓E1 | ✓E2 | ✓E1 | **E2** live (full `[clw] hit` = X4) |
| **Isolation / fence / secrets** (F-4.2, F-4.4, S7.1/2/10/11) | ✓E2 | ✓E1 | ✓**E2 live** | ✓E1 | **E2** (fence escape blocked live) |
| **Supply-chain** (F-4.5, S7.5) | ✓E0 | ✓E0 | ✓E0 | ✓E0 | **E0** oracle (X4 = live unpinned pull) |
| **Attestation / result-binding** (F-4.10, F-5.4, S7.3, S8.2) | ✓E2 | ✓E1 | ✓E1 | ✓E1 | **E2** (attestation/key live) |
| **Webhook / spawn / autoscaler** (F-5.8, F-7.1, S1.4.x, S7.9) | ✓E3 | ✓E1 | ✓E2 | ✓E5 | **E5** (spawn-fail→dead-letter) |
| **Auth gates (fail-closed)** (F-5.1, F-8.2, S7.4) | ✓E2 | ✓E2 | ✓**E2 fuzz** | ✓E2 | **E2** (21+ live auth-fuzz probes) |
| **Billing / metering** (F-5.6, S5.3.x, S14.x) | ✓E1 | ✓E1 | ✓E1 | ✓E1 | **E1** (push exporter armed-OFF = owner) |
| **Control-plane resilience** (F-7.2, F-5.7, S15.4) | ✓E2 | ✓E1 | ✓E2 | ✓**E5** | **E5** (fabricd kill → watchdog) |
| **Conformance / drift** (F-3.x, S16.x) | ✓E0 | ✓E0 | ✓E0 | ✓E0 | **E0** (17 vectors, both sides) |
| **CLI / SDK / CI shims** (F-9.x, S8.x, S9.x) | ✓E3 | ✓E1 | ✓E1 | ✓E1 | **E3** (live smoke) |
| **External customer / multi-tenant** (F-8.1, S1.1.x, S2.5.2, D1/D2) | — | — | — | — | **X4** (repo install click + 2nd tenant) |
| **Workspaces SKUs** (F-2.4, F-4.8, S3.x) | — | — | — | — | ⚫ campaign #2 (spine E0; SKUs planned) |

Full per-atom mapping: Appendix A (all 55 F-ids → suite + grade + disposition) and Appendix B
(all 155 S-ids → suite + grade + disposition). The **critic (§1/G1) is the machine proof** that this
table leaves no atom unmapped.

---

## 4. The completeness-critic (the machine that enforces G1)

```
for f in APPENDIX_A.f_ids (55):     assert coverage_map.cells_for(f).len >= 1
for s in APPENDIX_B.s_ids (155):    assert coverage_map.cells_for(s).len >= 1
for cell in coverage_map:           assert cell has {direction, grade, disposition, assertion, artifact_slot}
assert coverage_map.f_ids == ledger.f_ids   # no atom invented, none dropped
assert coverage_map.s_ids == ledger.s_ids
```
Red on any orphan. Runs in CI as a gate. This is why "complete" is a *provable* claim here, not a
promise.

---

## 5. Fabricable vs X4 — the honest split (no blur)

**Fabricable by me, to the grade shown** (the bulk): all of TS-1, TS-2 (dogfood-first-party), TS-3
(stress to cap+bit), TS-4 (chaos on dogfood — **owner run-go pending**), TS-5 (security-adversarial).

**X4-blocked — documented GAP with the exact unblock** (never claimed proven):
- **Real external customer** (F-8.1, S1.1.1–S1.1.8): a non-HuGR-Labs org must **install the App**
  — that install is an OAuth **UI click** that cannot be done headless. *Unblock:* 1 owner click, or a
  throwaway repo under a separate account (I can create the repo; the install click remains manual).
- **2nd real tenant** (D2, S5.2.x multi-tenant billing/fairness): tenant provisioning is **server-side**
  (corelink-server, cross-repo). *Unblock:* server-TL relay (owner is courier) or a real CoreLink
  onboarding.
- **Full moat `[clw] cache hit`** (S1.2.1 tail, S9.4): needs a real CoreLink PAT / real dispatch in the
  container runtime. Mint→redeem is proven live (E2); the *cache-hit* tail is X4.
- **GA billing usage-push** (F-5.6 exporters, S14.x history): armed **OFF** by owner/server-TL — COGS,
  low-urgency.

---

## 6. Run plan (respects the owner gate)

1. **This artifact** → owner reviews G1 (completeness) + G2 (evidence model). **← we are here.**
2. On go: **build** TS-1..TS-5 cells + the critic (techlead wave, disjoint by suite).
3. **Run TS-1/TS-2/TS-5** (non-disruptive) → evidence ledger.
4. **Run TS-3** stress to cap+bit → evidence ledger.
5. **Run TS-4 chaos** — *owner reviewed coverage first, per the gate* → evidence ledger.
6. **TS-6 external** — fabricate what's fabricable (throwaway repo), document the install-click + 2nd-tenant
   GAP with exact steps.
7. **Deliverable:** the evidence ledger — every atom → grade → cited artifact, + the ranked residual.

---

## Appendix A — the 55 feature atoms (F-ids)

Extracted verbatim from `FEATURES.md`. Each maps to `>= 1` suite cell (enforced by §4).

| F-id | badge | arm | primary suite | target grade | disposition |
|---|---|---|---|---|---|
| F-1.1 concurrency pricing | 🟢 | LIVE | TS-1/TS-2 | E3 | fabricable |
| F-1.2 no double-charge | 🟢 | LIVE | TS-1/TS-2 | E2 | fabricable (hit-tail X4) |
| F-1.3 5-tier ladder | 🟢 | LIVE | TS-1/TS-3 | E4 | fabricable |
| F-1.4 loss-impossible ceiling | 🟡 | DEFAULT-OFF | TS-1/TS-3 | E1 | fabricable (arm=owner) |
| F-1.5 entitlement | 🟡 | DEFAULT-OFF | TS-1 | E1 | fabricable |
| F-1.6 anti-abuse rails | 🟡 | LIVE/PLANNED | TS-3/TS-5 | E2 | fabricable (mining=planned) |
| F-2.1 direct front door | 🟢 | LIVE dogfood | TS-2 | E3 | fabricable (external=X4) |
| F-2.2 CoreLink memoized-check API/SDK | 🟢 | LIVE | TS-1 | E1 | fabricable through the direct CoreLink contract |
| F-2.3 `corelink run` | 🟢 | LIVE | TS-2 | E3 | fabricable |
| F-2.4 Workspaces door | ⚫ | PLANNED | TS-1 | E0 | campaign #2 |
| F-2.5 agent-exec seam | 🟡 | built | TS-1 | E1 | fabricable (e2e=X4) |
| F-3.1 wire types | 🟢 | LIVE | TS-1 | E0 | fabricable |
| F-3.2 API DTOs/errors | 🟢 | LIVE | TS-1/TS-5 | E1 | fabricable |
| F-3.3 conformance vectors | 🟢 | LIVE | TS-1 | E0 | fabricable |
| F-4.1 lease lifecycle | 🟢 | LIVE | TS-1/TS-2 | E2 | fabricable |
| F-4.2 isolation & security | 🟢 | LIVE | TS-5 | E2 | fabricable |
| F-4.3 cache-warm boot | 🟢 | LIVE | TS-2 | E2 | fabricable (hit=X4) |
| F-4.4 fence enforcement | 🟢 | LIVE | TS-5 | E2 | fabricable |
| F-4.5 supply-chain verify | 🟢 | LIVE | TS-5 | E0 | fabricable (live-pull=X4) |
| F-4.6 concurrency/expiry/recovery | 🟢 | LIVE | TS-4 | E5 | fabricable (chaos=owner-go) |
| F-4.7 Actions-YAML shim | 🟢 | LIVE/v0-sim | TS-1 | E0 | fabricable |
| F-4.8 workspace spine | 🟢 | LIVE spine | TS-1 | E0 | fabricable (SKUs=#2) |
| F-4.9 §13 envelope | 🟢 | LIVE | TS-1/TS-5 | E1 | fabricable |
| F-4.10 attestation signing | 🟢 | LIVE | TS-2 | E2 | fabricable |
| F-5.1 RunnerLease HTTP API | 🟢 | LIVE | TS-2/TS-5 | E2 | fabricable |
| F-5.2 admission/caps/fairness | 🟢/🟡 | reject-LIVE | TS-3 | E4 | fabricable |
| F-5.3 ledger & durable state | 🟢/🔵 | InMem-LIVE/pg | TS-4 | E2 | fabricable (pg=owner) |
| F-5.4 attestation/attested cost | 🟢 | LIVE | TS-2 | E2 | fabricable (consume=X4) |
| F-5.5 reaper & lifecycle | 🟢 | LIVE | TS-4 | E5 | fabricable (chaos=owner-go) |
| F-5.6 billing/metering | 🟡 | occupancy-LIVE | TS-1 | E1 | fabricable (push=owner) |
| F-5.7 multi-instance/shard | 🔵 | INERT@N=1 | TS-1 | E0 | owner-gated (RAISE-N) |
| F-5.8 runner-broker/autoscaler | 🟡 | DEFAULT-OFF | TS-2/TS-4 | E5 | fabricable |
| F-5.9 moat mint & cred-ticket | 🟢 | mint-LIVE | TS-2/TS-5 | E2 | fabricable (proven item-4) |
| F-5.10 multi-size resolver | ⚫ | INERT | TS-1 | E0 | owner-gated (taxonomy) |
| F-5.11 CoreLink seams | 🟡 | DEFAULT-OFF | TS-1 | E1 | fabricable (live=X4 PAT) |
| F-6.1 CloudflareEngine | 🟢 | LIVE | TS-2 | E2 | fabricable |
| F-6.2 NorthflankEngine | 🟡 | DEFAULT-OFF | TS-1 | E1 | fabricable |
| F-6.3 HybridBoxProvisioner | 🟡 | DEFAULT-OFF | TS-1 | E1 | fabricable |
| F-6.4 NoBox fail-closed | 🟢 | LIVE default | TS-1 | E1 | fabricable |
| F-6.5 spawn-worker contract | 🟢/⚪ | LIVE | TS-1/TS-2 | E2 | fabricable (TS-golden pending) |
| F-7.1 spawn-worker (prod) | 🟢 | LIVE | TS-2/TS-3 | E4 | fabricable |
| F-7.2 fabricd proxy Worker | 🟢 | LIVE singleton | TS-2/TS-4 | E5 | fabricable |
| F-7.3 canary alerting | 🟡 | BUILT/deploy-owner | TS-4 | E5 | fabricable (real breach=chaos) |
| F-8.1 external activation | 🔵 | OWNER-GATED | TS-6 | X4 | X4 (install click) |
| F-8.2 identity (ADR-0002) | 🟢/🔵 | server-LIVE | TS-2/TS-5 | E2 | fabricable (last-step=owner) |
| F-9.1 `corelink` CLI | 🟢 | LIVE | TS-2 | E3 | fabricable |
| F-9.2 GH Action + Buildkite | 🟢 | LIVE | TS-1 | E1 | fabricable |
| F-9.3 memoize composite | 🟢 | LIVE | TS-1 | E1 | fabricable |
| F-9.4 verify SDKs | 🟢 | LIVE | TS-1 | E0 | fabricable |
| F-9.5 check-exec-server | 🟢/🔵 | LIVE | TS-2 | E2 | fabricable (live-flip=owner) |
| F-10.1 golden counters (fabricd) | 🟢 | LIVE | TS-2 (evidence) | E2 | fabricable |
| F-10.2 golden counters (spawn) | 🟢 | LIVE | TS-2 (evidence) | E2 | fabricable |
| F-10.3 boot-honest diagnostics | 🟢 | LIVE | TS-2 | E2 | fabricable |
| F-10.4 quota-headroom monitor | 🟡 | DEFAULT-OFF | TS-1 | E1 | fabricable |
| F-10.5 runbooks/CI/image build | 🟢 | LIVE | TS-1 | E0 | fabricable |

## Appendix B — the 155 scenario atoms (S-ids), by persona

Extracted verbatim from `USE-SCENARIOS.md`. Per-persona coverage + the explicit X4/owner call-outs.
The critic (§4) enforces **per-S-id** cell existence; this table is the human-readable disposition.

| Persona | S-ids | primary suite | fabricable live | X4 / owner-gated (the honest residual) |
|---|---|---|---|---|
| P1 dev (46) | S1.1.1–S1.7.5 | TS-2/TS-3 | dogfood job, warm/cold boot, matrix, ceiling, recovery | external install (S1.1.1/2), full cache-hit smoke (S1.2.1 tail), multi-size (S1.3.3/4), GPU/arch (S1.3.4) |
| P2 CoreLink check/SDK user (10) | S2.1.1–S2.5.2 | TS-1 | contract/attested-cost cells | direct CoreLink CLI/SDK fixture |
| P3 workspaces (7) | S3.1–S3.7 | TS-1 | spine E0 | **campaign #2** (SKUs planned) |
| P4 agent (7) | S4.1–S4.7 | TS-1/TS-5 | isolation, attested metrics, contention | full agent-loop e2e = X4 until a direct CoreLink CLI/SDK fixture is armed |
| P5 operator (18) | S5.1.1–S5.5.2 | TS-2/TS-4 | counters, boot-diag, egress-cut, image-upgrade, chaos-recovery | RAISE-N (S5.2.2/4), vCPU wall arm (S5.3.2), multi-region (S5.5.x) |
| P6 buyer (4) | S6.1–S6.4 | TS-1 | mechanism-proven | pricing/positioning = owner (GA) |
| P7 attacker (15) | S7.1–S7.15 | **TS-5** | fence escape, secret exfil, forge, cross-tenant, supply-chain, replay, net_policy, single-use — **live adversarial** | IMDS/G2 probe (S7.6) tracked-gap |
| P8 power-user (6) | S8.1–S8.6 | TS-2 | `corelink run` live smoke + exit contract | full live-fabric edge (S8.4 tail) |
| P9 migrator (5) | S9.1–S9.5 | TS-2 | label-matcher, fail-open, hybrid | bake-off full smoke (S9.1), Stage-C decommission (S9.5) |
| P10 compliance (6) | S10.1–S10.6 | TS-1 | attestation/audit-evidence | region/DPA/GDPR/SSO = owner (legal/M3) |
| P11 support (5) | S11.1–S11.5 | TS-2 | recovery, self-diag, attested escalation | full cache-hit smoke (S11.1 tail) |
| P12 finance (4) | S12.1–S12.4 | TS-1 | vCPU/cost signal | GA billing/tiering = owner |
| P13 lifecycle (5) | S13.1–S13.5 | TS-2 | uninstall fail-safe, re-onboard | tier up/down + GDPR-delete = owner (GA) |
| P14 usage-API (6) | S14.1–S14.6 | TS-2 | `GET /v1/leases/{id}` live; usage/history routes | full history (S14.2/6) = billing-push owner |
| P15 SRE (5) | S15.1–S15.5 | TS-2/TS-4 | mint/spawn counters, restart-honesty | N>1 flap (S15.4) = RAISE-N |
| P16 contract (3) | S16.1–S16.3 | TS-1 | drift tripwire, transcription law | new-vector coordination (S16.3) = cross-repo |
| P17 comms (3) | S17.1–S17.3 | TS-1 | signal substrate | statuspage/comms/i18n = owner (surface) |

**Totals:** 55 F-ids + 155 S-ids = **210 atoms**, all mapped. Deterministic disposition (by ledger
badge): **163 fabricable** (🟢 97 + 🟡 66) · **43 owner/external-gated** (🔵) · **2 X4-pure** (⚪) ·
**2 planned/#2** (⚫). The load-bearing external dependencies inside the 🔵 43 are: the external-customer
install click, the 2nd real tenant, the full cache-hit smoke, and the GA-billing / RAISE-N owner flips.

---

*Change log — 2026-07-19 v1: initial spec, atom inventory frozen (55 F + 155 S), coverage matrix +
completeness-critic defined, fabricable/X4 split made explicit. Awaiting owner G1/G2 review + run-go.*
