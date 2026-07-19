# `scripts/e2e/` — the E2E validation suite (real user in prod)

The executable side of [`docs/validation/E2E-SUITE-SPEC.md`](../../docs/validation/E2E-SUITE-SPEC.md).
It plays a **real user against the live fabric**, with the same powers and limits a user has, and holds
two guarantees the owner demanded:

- **G1 — completeness (machine-proven).** Every product atom — **55 features** + **155 scenarios**,
  parsed live from the ledger — is bound to `>= 1` test cell. The **completeness-critic** fails the build
  on any orphan. Coverage cannot silently rot.
- **G2 — behavioral evidence, not status codes.** Every cell is a triple
  `stimulus → behavioral assertion → captured artifact`. A `200` that behaves wrong is a FAIL.

## Layout

| File | Role |
|---|---|
| `ledger.mjs` | Parses `docs/product/{FEATURES,USE-SCENARIOS}.md` H3 cards → the atom universe. No hand-copy → no drift. |
| `coverage-map.mjs` | The declared cells: each binds atoms (by id / prefix) to a suite · directions · grade · disposition. |
| `completeness.test.mjs` | **The critic (G1).** `node --test`, zero deps. Expands prefixes vs the live ledger, asserts full coverage + no dead refs + well-formed cells. |
| `run.sh` | Single entrypoint: `critic`, `ts1`..`ts5`, `ts4` (chaos, owner-gated), `all`. |
| `journey/ stress/ security/ chaos/` | Suite cells (authored in the build wave). |

## Run

```sh
scripts/e2e/run.sh critic      # G1 gate — fast, zero-infra
node --test scripts/e2e/completeness.test.mjs   # same, direct
```

## The evidence model (G2) — the contract every cell obeys

A cell is not "green" on an HTTP code. It must assert the **behavior** and **capture the artifact**:

- **state transition** — lease `pending→held→closed`; PAT `minted→redeemed→revoked`
- **counter delta** — the exact golden-signal seam moved by exactly N (no over/under-count)
- **log line** — the loud log on the critical path actually fired
- **side-effect presence/absence** — box torn down; secret **absent** from env/log/body
- **security invariant** — fail-closed 401/404; forced server-side `net_policy`; single-use ticket

Artifacts land in `docs/validation/evidence/<run-id>/` keyed by atom-id.

## The four directions

Each cell declares the directions its nature supports — **happy · edge · adversarial · failure**. Not
every atom admits all four (a pure conformance vector has no failure-injection); the cell is honest about
which apply. The critic enforces *atom* coverage; direction coverage is the suite-authoring checklist.

## Real-user fidelity (the "same powers and limits" law)

TS-2/3/5 drive **only** what a real user can reach — the public `/v1` API with a real PAT, the GitHub App
path, the `corelink` CLI — on the live **dogfood** install (App `144561227`, first-party). The privileged
`/internal` surface is read **only for evidence** (counters), never as a stimulus a user couldn't produce.
A real *external* customer (2nd org install click, 2nd tenant, full `[clw] cache hit`) is **TS-6 / X4** —
proven where fabricable, honestly gated where not.

## Disposition (deterministic, by ledger badge; 2026-07-19)

**163 fabricable** (🟢 97 + 🟡 66) · **43 owner/external-gated** (🔵) · **2 X4-pure** (⚪) ·
**2 planned/#2** (⚫). The load-bearing external dependencies live inside the 🔵 43.
